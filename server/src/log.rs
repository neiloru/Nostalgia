//! Console log lines in the same format as the Node.js server: `2026-11-08 20:04:09 INFO  message`.

fn ts() -> String {
    let s = nostalgia_shared::iso_now();
    s.replace('T', " ")[..19].to_string()
}

pub fn info(msg: impl AsRef<str>) {
    println!("{} INFO  {}", ts(), msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    eprintln!("{} WARN  {}", ts(), msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    eprintln!("{} ERROR {}", ts(), msg.as_ref());
}
