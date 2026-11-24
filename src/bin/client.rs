use std::error::Error;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
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
    /// Print the last traced values to either standard output or a file, then quit.
    Dump { 
        /// Path of the file to write the trace output to. Defaults to standard output. 
        path: Option<String>
    },
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

    let mut sock = UnixStream::connect(sock_path)?;
    sock.set_read_timeout(Some(std::time::Duration::from_millis(200)));
    sock.set_nonblocking(false);

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting termination handler");

    let mut buf = [0u8; 4096]; //vec![0; 4096];
    let mut buf2 = Vec::<u8>::new();
    let mut component_cached = "".to_string();

    let mut output: std::io::BufWriter<Box<dyn std::io::Write>> = match args.mode {
        Mode::Dump { path: Some(ref path) } => {
            let f = std::fs::File::create(path)?;
            std::io::BufWriter::new(Box::new(f))
        }
        _ => std::io::BufWriter::new(Box::new(std::io::stdout())),

    };

    while running.load(Ordering::SeqCst) {
        buf.fill(0);
        match sock.read(&mut buf) {
            Ok(bytes) => {
                if bytes == 0 {
                    eprintln!("Connection interrupted.");
                    break;
                }
                // split incoming buf into messages by '\n'
                // append current portion to buf2
                // if portion has no terminating '\n', break
                // make a str out of buf2 and parse that
                //
                // Fold into a vec of vecs? a vec of slices?
                // Can probably use the accumulator as an index of sorts
                // like, fold( (0, 0), |(index, length), x| {
                // length+=1
                // if x is '\n', push &buf[index..index+length] somewhere
                // and also (index, length) = (index += length, 0)
                // })
                // after fold, if bytes is max, manually add the last slice to the slice list
                // then do the above
                let (last_index, last_length) = buf.iter().fold( (0, 0), |(index, length), c| {
                    if *c == 10 { // newline
                        buf2.extend(&buf[index..(index+length+1)]);
                        let msg = str::from_utf8(&buf2);
                        if let Ok(line) = msg {
                            //eprint!("Line! §{line}§");
                            let parts: Vec<_> = line.splitn(3, "\t").collect();
                            let (time, component, message) = match parts.len() {
                                1 => ("", "", parts[0]),
                                2 => (parts[0], "", parts[1]),
                                3 => {
                                    component_cached = parts[1].to_string();
                                    (parts[0], parts[1], parts[2])
                                }
                                _ => unreachable!(),
                            };
                            let message = message.trim();
                            let time: Option<String> = match i128::from_str(time) {
                                Ok(0) => match message {
                                    "EOP" => match args.mode {
                                        Mode::Attach => {
                                            None
                                        }
                                        Mode::Dump {path: _} => {
                                            running.store(false, Ordering::SeqCst);
                                            None
                                        }
                                    },
                                    "TERM" => {
                                        running.store(false, Ordering::SeqCst);
                                        None
                                    }
                                    _ => {
                                        None
                                    }
                                },
                                Ok(n) => match time::Timestamp::from_nanoseconds(n) {
                                    Ok(t) => match t.format(time::macros::format_description!(
                                        "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:6]"
                                    )) {
                                        Ok(s) => Some(s),
                                        Err(_) => Some("".to_string()),
                                    },
                                    Err(_) => Some("".to_string()),
                                },
                                Err(_) => Some("".to_string()),
                            };
                            if let Some(time) = time
                                && (args.filter.is_empty() ||
                                    args.filter.iter().any(|p|
                                        if component.is_empty() {
                                            component_cached.as_str()
                                        } else {
                                            component
                                        }.matches(p).next().is_some())
                                ) {
                                    writeln!(&mut output, "{time}\t{component}\t{message}");
                                    //println!("{time}\t{component}\t{message}");
                            }
                        } else {
                            eprintln!("Malformed UTF-8");
                            // TODO: print better error
                        }
                        buf2.clear();
                        (index + length + 1, 0)
                    } else if *c == 0 {
                        (index + length + 1, 0)
                    } else {
                        (index, length + 1)
                    }
                });
                if last_length > 0 {
                    let slice = &buf[last_index..last_index + last_length];
                    buf2.extend(slice);
                }
                output.flush();
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
    //std::io::stdout().flush();
    sock.shutdown(std::net::Shutdown::Both);
    Ok(())
}
