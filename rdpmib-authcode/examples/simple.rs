use std::{env, thread};

use anyhow::bail;
use rdpmib_authcode::EventLoop;

fn main() -> anyhow::Result<()> {
    let Some(url) = env::args().nth(1) else {
        bail!("no args");
    };

    let event_loop = EventLoop::new()?;
    let dispatcher = event_loop.dispatcher();

    let _ = thread::spawn(move || {
        let result = dispatcher.request_get_code(&url).unwrap();
        println!("{result}");
    });

    event_loop.run().unwrap();
    Ok(())
}
