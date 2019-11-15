//! We want to present a nice message to users when lorri crashes. The `human_panic` crate does
//! just that, with one caveat: it currently swallows the panic message:
//! https://github.com/rust-cli/human-panic/issues/55. That makes it unusable for us.
//!
//! This file is an alternative implementation of `human_panic` where the reported error includes
//! the panic message. It is lifted almost verbatim from
//! https://github.com/foundpatterns/torchbear/commit/83423689d2b9e39dbd0cdc841399e9d769f23f65.
//!
//! TODO: use `human_panic` once https://github.com/rust-cli/human-panic/issues/55 is resolved.

use backtrace::Backtrace;
use std::{collections::HashMap, env};

use std::io::Write;

/// Install the panic hook.
pub fn install_hook() {
    let mut data = std::collections::HashMap::new();
    data.insert("%NAME%", env!("CARGO_PKG_NAME"));
    data.insert("%GITHUB%", env!("CARGO_PKG_REPOSITORY"));
    create_hook(r#"
        Well, this is embarrassing...
        %NAME% had a problem and crashed. To help us diagnose the problem, you can send us a crash report.
        We have generated a report file at "%PATH%". Submit an issue with the subject of "%NAME% Crash Report"
        and include the report as an attachment.
        - Github: %GITHUB%"#, Some(data), |path, data| {
            if let Some(path) = path {
                let mut fs = std::fs::File::create(path)?;
                fs.write_all(data.as_bytes())?;
            }
            Ok(())
        });
}

// 100% copied from
// https://github.com/foundpatterns/torchbear/commit/83423689d2b9e39dbd0cdc841399e9d769f23f65
fn create_hook<F>(text: &'static str, data: Option<HashMap<&'static str, &'static str>>, f: F)
where
    F: 'static + Fn(Option<::std::path::PathBuf>, String) -> std::io::Result<()> + Send + Sync,
{
    match ::std::env::var("RUST_BACKTRACE") {
        Err(_) => {
            let data = data.unwrap_or({
                let mut data = HashMap::new();
                data.insert("%NAME%", env!("CARGO_PKG_NAME"));
                data.insert("%GITHUB%", env!("CARGO_PKG_REPOSITORY"));
                data
            });

            std::panic::set_hook(Box::new(move |info: &std::panic::PanicInfo| {
                let mut text = String::from(text);

                for (k, v) in &data {
                    text = text.replace(k, v);
                }

                let path = if text.contains("%PATH%") {
                    let tmp = env::temp_dir().join(format!(
                        "report-{}.log",
                        ::uuid::Uuid::new_v4().to_hyphenated().to_string()
                    ));
                    text = text.replace("%PATH%", tmp.to_string_lossy().as_ref());
                    Some(tmp)
                } else {
                    None
                };

                println!("{}", text);

                let mut payload = String::new();

                let os = if cfg!(target_os = "windows") {
                    "Windows"
                } else if cfg!(target_os = "linux") {
                    "Linux"
                } else if cfg!(target_os = "macos") {
                    "Mac OS"
                } else if cfg!(target_os = "android") {
                    "Android"
                } else {
                    "Unknown"
                };

                payload.push_str(&format!("Name: {}\n", env!("CARGO_PKG_NAME")));
                payload.push_str(&format!("Version: {}\n", env!("CARGO_PKG_VERSION")));
                payload.push_str(&format!("Operating System: {}\n", os));

                // Actually include the message!
                payload.push_str(&format!(
                    "Message: {}\n",
                    info.message().unwrap_or(&format_args!("(none)"))
                ));

                if let Some(inner) = info.payload().downcast_ref::<&str>() {
                    payload.push_str(&format!("Cause: {}.\n", &inner));
                }

                match info.location() {
                    Some(location) => payload.push_str(&format!(
                        "Panic occurred in file '{}' at line {}\n",
                        location.file(),
                        location.line()
                    )),
                    None => payload.push_str("Panic location unknown.\n"),
                };

                payload.push_str(&format!("{:#?}\n", Backtrace::new()));

                f(path, payload).expect("Error generating report")
            }));
        }
        Ok(_) => {}
    };
}
