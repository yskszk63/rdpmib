use std::thread;

use rdpmib_authcode::EventLoop;

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    let dispatcher = event_loop.dispatcher();

    let _ = thread::spawn(move || {
        rdpmib_dbus::run(move |url| {
            let code = dispatcher
                .request_get_code(&url)
                .map_err(|e| e.to_string())?;
            Ok(code)
        })
        .unwrap();
    });

    event_loop.run()?;
    Ok(())
}
