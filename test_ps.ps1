use std::process::{Command, Stdio};
use std::io::BufRead;

fn main() {
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "dir"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn");

    if let Some(stdout) = child.stdout.take() {
        let reader = std::io::BufReader::new(stdout);
        for (i, line) in reader.lines().enumerate() {
            match line {
                Ok(l) => println!("LINE {}: {}", i, l),
                Err(e) => println!("ERROR: {}", e),
            }
        }
    }

    let status = child.wait().unwrap();
    println!("Exit code: {:?}", status.code());
}
