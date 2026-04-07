use std::sync::mpsc;

use tao::event::Event;
use tao::event::WindowEvent;
use tao::event_loop::ControlFlow;
use tao::event_loop::EventLoopBuilder;
use tao::event_loop::EventLoopProxy;
use tao::event_loop::EventLoopWindowTarget;
use tao::platform::unix::WindowExtUnix;
use tao::window::Window;
use tao::window::WindowBuilder;
use thiserror::Error;
use wry::PageLoadEvent;
use wry::WebContext;
use wry::WebView;
use wry::WebViewBuilder;
use wry::WebViewBuilderExtUnix;

use crate::AuthCodeError;

use super::Dispatcher;
use super::event::UserEvent;

#[derive(Debug, Error)]
pub enum EventLoopError {}

type Result<T> = std::result::Result<T, EventLoopError>;

enum State {
    Begin {
        url: String,
        reply: mpsc::Sender<std::result::Result<String, AuthCodeError>>,
    },
    Login {
        url: String,
        reply: mpsc::Sender<std::result::Result<String, AuthCodeError>>,
        window: Window,
        webview: WebView,
    },
    GetCode {
        url: String,
        reply: mpsc::Sender<std::result::Result<String, AuthCodeError>>,
        _window: Window,
        webview: WebView,
    },
}

const LOGIN_URL: &'static str = "https://login.microsoftonline.com/";

fn handle_begin(
    state: State,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
) -> std::result::Result<State, AuthCodeError> {
    let State::Begin { url, reply } = state else {
        unreachable!()
    };

    let window = WindowBuilder::new()
        .with_title(env!("CARGO_PKG_NAME"))
        .build(&event_loop)?;
    let vbox = window.default_vbox().unwrap();

    let dirs = xdg::BaseDirectories::with_prefix("rdpmib");
    let dir = dirs.create_data_directory("webkit")?;
    let mut web_cx = WebContext::new(Some(dir));
    let webview = {
        let proxy = proxy.clone();
        WebViewBuilder::new_with_web_context(&mut web_cx)
            .with_url(LOGIN_URL)
            .with_on_page_load_handler(move |event, url| {
                if !matches!(event, PageLoadEvent::Started) {
                    return;
                }

                proxy
                    .send_event(UserEvent::PageLoading(url))
                    .expect("event loop closed");
            })
            .build_gtk(vbox)?
    };

    Ok(State::Login {
        url,
        reply,
        window,
        webview,
    })
}

fn handle_state_changed(
    state: &mut Option<State>,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<()> {
    if state.is_none() {
        return Ok(());
    }

    match state.as_ref().unwrap() {
        State::Begin { reply, .. } => {
            let reply = reply.clone();
            let current = state.take().unwrap();
            let next = match handle_begin(current, event_loop, proxy) {
                Ok(next) => {
                    proxy
                        .send_event(UserEvent::StateChanged)
                        .expect("event loop is not closed");
                    Some(next)
                }
                Err(err) => {
                    // suppress if peer is closed.
                    reply.send(Err(err)).ok();
                    None
                }
            };
            *state = next;
        }

        State::Login { .. } => {}

        State::GetCode {
            webview,
            url,
            reply,
            ..
        } => {
            let Err(err) = webview.load_url(url) else {
                return Ok(());
            };
            // suppress if peer is closed.
            reply.send(Err(err.into())).ok();
        }
    }

    Ok(())
}

fn handle_page_loading_login(state: State, url: &str) -> std::result::Result<State, AuthCodeError> {
    let url = url::Url::parse(&url)?;

    let login_url_origin = url::Url::parse(LOGIN_URL).expect("must parse").origin();
    let origin = url.origin();
    if login_url_origin == origin {
        return Ok(state);
    }

    let State::Login {
        url,
        reply,
        window,
        webview,
    } = state
    else {
        unreachable!()
    };

    Ok(State::GetCode {
        url,
        reply,
        _window: window,
        webview,
    })
}

fn handle_page_loading_get_code(url: &str) -> std::result::Result<Option<String>, AuthCodeError> {
    let url = url::Url::parse(&url)?;

    for (key, val) in url.query_pairs() {
        match key.as_ref() {
            "code" => return Ok(Some(val.to_string())),
            "err" => {
                return Err(AuthCodeError::Failed(val.to_string()));
            }
            _ => {}
        }
    }

    Ok(None)
}

fn handle_page_loading(
    state: &mut Option<State>,
    proxy: &EventLoopProxy<UserEvent>,
    url: &str,
) -> Result<()> {
    match state.as_ref().expect("state must present.") {
        State::Begin { .. } => unreachable!(),

        State::Login { reply, .. } => {
            let reply = reply.clone();
            let current = state.take().expect("must present");
            let next = match handle_page_loading_login(current, url) {
                Ok(next) => {
                    proxy
                        .send_event(UserEvent::StateChanged)
                        .expect("event loop closed");
                    Some(next)
                }
                Err(err) => {
                    // suppress if peer is closed.
                    reply.send(Err(err.into())).ok();
                    None
                }
            };
            *state = next;
        }

        State::GetCode { reply, .. } => {
            let reply = reply.clone();
            match handle_page_loading_get_code(url) {
                Ok(None) => {}
                Ok(Some(code)) => {
                    // suppress if peer is closed.
                    reply.send(Ok(code)).ok();
                    *state = None;
                }
                Err(err) => {
                    // suppress if peer is closed.
                    reply.send(Err(err.into())).ok();
                }
            };
        }
    }

    Ok(())
}

fn handle_user_event(
    state: &mut Option<State>,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
    event: UserEvent,
) -> Result<()> {
    match event {
        UserEvent::StateChanged => {
            handle_state_changed(state, event_loop, proxy)?;
        }

        UserEvent::RequestGetCode(url, reply) => {
            if state.is_some() {
                // suppress if peer is closed.
                reply.send(Err(AuthCodeError::Busy)).ok();
                return Ok(());
            }

            *state = Some(State::Begin { url, reply });
            proxy
                .send_event(UserEvent::StateChanged)
                .expect("event loop closed");
        }

        UserEvent::PageLoading(url) => {
            handle_page_loading(state, proxy, &url)?;
        }
    }

    Ok(())
}

fn handle_event(
    state: &mut Option<State>,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
    event: Event<UserEvent>,
) -> Result<()> {
    match event {
        Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } => {
            *state = None;
        }

        Event::UserEvent(event) => {
            handle_user_event(state, event_loop, proxy, event)?;
        }

        _ => {}
    }

    Ok(())
}

pub struct EventLoop {
    event_loop: tao::event_loop::EventLoop<UserEvent>,
    proxy: EventLoopProxy<UserEvent>,
}

impl EventLoop {
    pub fn new() -> Result<Self> {
        let event_loop = EventLoopBuilder::with_user_event().build();
        let proxy = event_loop.create_proxy();

        Ok(Self { event_loop, proxy })
    }

    pub fn dispatcher(&self) -> Dispatcher {
        let proxy = self.proxy.clone();
        Dispatcher::new(proxy)
    }

    pub fn run(self) -> Result<()> {
        let mut state = None;
        let proxy = self.proxy.clone();
        self.event_loop.run(move |event, event_loop, control_flow| {
            *control_flow = ControlFlow::Wait;

            let Err(err) = handle_event(&mut state, event_loop, &proxy, event) else {
                return;
            };

            eprintln!("{err}");
            *control_flow = ControlFlow::Exit;
        });
    }
}
