use std::error::Error;
use std::io::Write;
use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixDatagram, UnixListener, UnixStream};

mod util;

struct Message {
    time: std::time::Duration,
    content: String
}

impl Message {
    const EMPTY: Message = Message { time: std::time::Duration::ZERO, content: String::new() };
}

impl std::fmt::Display for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        writeln!(f, "{:0<19}\t{}", self.time.as_nanos(), self.content)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let dir_env = std::env::var("OVERDIGIT_CODESYS_SOCK_DIR");
    let env_set = dir_env.is_ok();
    let dir = dir_env.unwrap_or("/var/opt/codesyscontrolapi/extfuncs/".to_string());

    let mut buf = vec![0; 1024];
    let mut log = std::collections::VecDeque::<Message>::new();

    let dir_path = std::path::Path::new(&dir);
    if !dir_path.is_dir() {
        if env_set {
            return Err(
                format!("Invalid path - \"{dir}\" does not exist or is not a directory").into(),
            );
        }
        let path = std::path::Path::new(&dir);
        util::wait_for(path)?;
    }
    let sock_path = dir_path.join("overdigit-trace-socket.sock");
    match sock_path.try_exists() {
        Ok(true) => {
            match UnixStream::connect(&sock_path) {
                Ok(stream) => {
                    // Socket is in use
                    stream.shutdown(std::net::Shutdown::Both)?;
                    // Gonna let the ::bind call throw an error instead
                }
                Err(_e) => {
                    // Socket is not in use or at the very least not connectable, safe to remove
                    std::fs::remove_file(&sock_path)?;
                }
            }
        }
        Ok(false) => {}
        Err(e) => return Err(Box::new(e)),
    }

    let socket_user =
        std::env::var("OVERDIGIT_CODESYS_SOCK_USER").unwrap_or("codesyscontrol".to_string());
    let socket_uid = util::get_uid(&socket_user)?.unwrap_or_else(|| {
        panic!(
            "{}",
            format!("Could not find user {socket_user} in /etc/passwd.")
        )
    });

    let log_sock = UnixDatagram::bind(&sock_path)?;

    std::os::unix::fs::chown(&sock_path, Some(socket_uid), None)?;
    std::fs::set_permissions(&sock_path, Permissions::from_mode(0o775))?;

    log_sock.set_read_timeout(Some(std::time::Duration::from_millis(100)));

    let dump_dir_env = std::env::var("OVERDIGIT_EXPORT_SOCK_DIR");
    let dump_dir = dump_dir_env.unwrap_or("/var/run/".to_string());


    let dump_dir_path = std::path::Path::new(&dump_dir);
    if !dump_dir_path.is_dir() {
        return Err(
            format!("Invalid path - \"{dump_dir}\" does not exist or is not a directory").into(),
        );
    }
    let dump_sock_path = dump_dir_path.join("overdigit-trace-socket.sock");
    match dump_sock_path.try_exists() {
        Ok(true) => {
            match UnixStream::connect(&dump_sock_path) {
                Ok(stream) => {
                    // Socket is in use
                    stream.shutdown(std::net::Shutdown::Both)?;
                    // Gonna let the ::bind call throw an error instead
                }
                Err(_e) => {
                    // Socket is not in use or at the very least not connectable, safe to remove
                    std::fs::remove_file(&dump_sock_path)?;
                }
            }
        }
        Ok(false) => {}
        Err(e) => return Err(Box::new(e)),
    }

    let listener = UnixListener::bind(&dump_sock_path)?;
    listener.set_nonblocking(true);
    let mut readers = Vec::<std::os::unix::net::UnixStream>::new();


    loop {
        match log_sock.recv(&mut buf) {
            Ok(bytes) => {
                buf[bytes] = 0; // Terminate the string
                let msg =
                    String::from_utf8((buf[..bytes]).to_vec()).unwrap_or("[ERR]\t\tInvalid string".to_string()).trim_end_matches('\n').to_string();
                let item = Message {time: std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).unwrap(), content: msg};
                //println!("{item:?}");
                readers.retain(|mut stream| {
                    match stream.write_all(format!("{}", item).as_bytes()) {
                        Ok(()) => {true}
                        Err(e) => {
                            match e.kind() {
                                _ => {
                                    eprintln!("{e:?}");
                                    false
                                }
                            }
                        }
                    }
                });
                log.push_back(item);
                log.retain_back(10_000);
            }
            Err(e) => match e.kind() {
                std::io::ErrorKind::WouldBlock => {}
                _ => eprintln!("{e:?}"),
            },
        }
        loop {
            match listener.accept() {
                Ok((mut sock, _addr)) => {
                    for item in &log {
                        match sock.write_all(format!("{}", item).as_bytes()) {
                            Ok(()) => {}
                            Err(e) => {
                                eprintln!("{e:?}");
                                break;
                            }
                        }
                    }
                    sock.write_all(format!("{}", Message::EMPTY).as_bytes());
                    readers.push(sock);
                    // TODO: send all messages
                    // TODO: then send empty message with timestamp zero (end of past marker)
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::WouldBlock => break,
                    _ => eprintln!("{e:?}"),
                }
            }
        }
    }
    Ok(())
}
