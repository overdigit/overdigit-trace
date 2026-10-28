use std::error::Error;
use std::fs::Permissions;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Parser;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path of the socket to connect to.
    #[arg(short, long, default_value_t = "/var/run/overdigit-trace-socket.sock".to_string())]
    socket: String,
    /// Filter to match the component sending a message against; Can be specified multiple times
    /// for multiple filters.
    #[arg(short, long)]
    filter: Vec<String>,
    #[command(subcommand)]
    /// Mode of operation.
    mode: Mode,
}

#[derive(Parser)]
enum Mode {
    /// Print the last traced values and continue printing in real time.
    Attach,
    /// Print the last traced values, then quit.
    Dump,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let sock_path = std::path::Path::new(&args.socket);
    match sock_path.try_exists() {
        Ok(true) => {}
        Ok(false) => {
            // TODO: proper error + usage here
            return Err("Socket does not exist".into());
        }
        Err(e) => return Err(Box::new(e)),
    }

    let mut sock = UnixStream::connect(&sock_path)?;
    sock.set_read_timeout(Some(std::time::Duration::from_millis(200)));
    //listener.set_nonblocking(true);

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting termination handler");

    let mut buf = vec![0; 4096];

    while running.load(Ordering::SeqCst) {
        match sock.read(&mut buf) {
            Ok(bytes) => {
                println!("Packet!");
                if bytes < 4096 {
                    buf[bytes] = 0; // Terminate the string
                }
                let msg = str::from_utf8(&buf[..bytes])
                    .unwrap_or("\t[Error]\tInvalid string")
                    .trim_end_matches('\n');
                //.to_string();
                for line in msg.split("\n") { // TODO: split_inclusive, if last isn't newline
                                              // terminated put in temp buffer and append the next
                                              // read to it 
                    println!("Line!");
                    let parts: Vec<_> = line.splitn(3, "\t").collect();
                    let (time, component, message) = match parts.len() {
                        1 => ("", "", parts[0]),
                        2 => (parts[0], "", parts[1]),
                        3 => (parts[0], parts[1], parts[2]),
                        _ => unreachable!(),
                    };
                    let time = match i128::from_str(time) {
                        Ok(0) => match message {
                            "EOP" => match args.mode {
                                Mode::Attach => continue,
                                Mode::Dump => {
                                    running.store(false, Ordering::SeqCst);
                                    continue;
                                }
                            },
                            "TERM" => {
                                running.store(false, Ordering::SeqCst);
                                continue;
                            }
                            _ => continue,
                        },
                        Ok(n) => match time::Timestamp::from_nanoseconds(n) {
                            Ok(t) => match t.format(time::macros::format_description!(
                                "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:6]"
                            )) {
                                Ok(s) => s,
                                Err(_) => "".to_string(),
                            },
                            Err(_) => "".to_string(),
                        },
                        Err(_) => "".to_string(),
                    };
                    if args.filter.len() > 0 && !args.filter.iter().any(|p| component.matches(p).next().is_some()) {
                        continue;
                    }
                    println!("!{time}!\t\"{component}\"\t${message}$");
                }
            }
            Err(e) => {
                match e.kind() {
                    std::io::ErrorKind::Interrupted => {}
                    std::io::ErrorKind::WouldBlock => {}
                    _ => eprintln!("{e:?}"),
                };
            }
        };
    }
    std::io::stdout().flush();
    sock.shutdown(std::net::Shutdown::Both);
    Ok(())
}
