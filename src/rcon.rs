use anyhow::{anyhow, bail, Context, Result};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const TYPE_RESPONSE: i32 = 0;
const TYPE_COMMAND: i32 = 2;
const TYPE_AUTH: i32 = 3;

/// The server splits responses into fragments of this many UTF-16 units.
const FRAGMENT_CHARS: usize = 4096;
/// The server reads at most 1460 bytes per request, 14 of which are framing.
const MAX_COMMAND_BYTES: usize = 1446;
const MAX_PACKET_BYTES: usize = 1 << 20;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Generous, as commands like `locate` or `save-all flush` block for a while.
const IO_TIMEOUT: Duration = Duration::from_secs(60);

struct Packet {
    id: i32,
    kind: i32,
    body: Vec<u8>,
}

/// Minimal RCON client that, unlike most, reassembles fragmented responses.
pub struct Client {
    address: String,
    password: String,
    stream: Option<TcpStream>,
    next_id: i32,
}

impl Client {
    pub fn connect(address: &str, password: &str) -> Result<Self> {
        let mut client = Client {
            address: address.to_string(),
            password: password.to_string(),
            stream: None,
            next_id: 1,
        };
        client.reconnect()?;
        Ok(client)
    }

    fn reconnect(&mut self) -> Result<()> {
        self.stream = None;
        let mut last_error = anyhow!("address resolved to nothing");
        let addrs = self
            .address
            .to_socket_addrs()
            .with_context(|| format!("invalid address '{}'", self.address))?;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(IO_TIMEOUT))?;
                    stream.set_write_timeout(Some(IO_TIMEOUT))?;
                    stream.set_nodelay(true)?;
                    self.stream = Some(stream);
                    return self.authenticate().inspect_err(|_| self.stream = None);
                }
                Err(e) => last_error = e.into(),
            }
        }
        Err(last_error.context(format!("failed to connect to {}", self.address)))
    }

    fn authenticate(&mut self) -> Result<()> {
        let id = self.send(TYPE_AUTH, &self.password.clone())?;
        loop {
            let packet = self.read()?;
            // Source-style servers send an empty response before the verdict.
            if packet.kind != TYPE_COMMAND {
                continue;
            }
            if packet.id != id {
                bail!("authentication failed: wrong RCON password");
            }
            return Ok(());
        }
    }

    /// Runs a command and returns its whole output.
    ///
    /// After a connection failure the next call reconnects first. A failed
    /// command is never resent, as it may have run already.
    pub fn command(&mut self, command: &str) -> Result<String> {
        if command.len() > MAX_COMMAND_BYTES {
            bail!(
                "command is too long for RCON ({} bytes, the limit is {MAX_COMMAND_BYTES})",
                command.len()
            );
        }
        if self.stream.is_none() {
            self.reconnect()?;
        }
        self.exchange(command).inspect_err(|_| self.stream = None)
    }

    fn exchange(&mut self, command: &str) -> Result<String> {
        let id = self.send(TYPE_COMMAND, command)?;
        let mut body = self.read_reply(id)?.body;

        // A full fragment means more may follow, and nothing marks the last
        // one. So ask for something else and read until *its* reply arrives.
        // Only sent once the first fragment is in: the server drops the
        // connection if two requests land in the same read.
        if String::from_utf8_lossy(&body).encode_utf16().count() >= FRAGMENT_CHARS {
            let sentinel = self.send(TYPE_RESPONSE, "")?;
            loop {
                let packet = self.read()?;
                if packet.id == sentinel {
                    break;
                }
                body.extend(packet.body);
            }
        }
        Ok(String::from_utf8_lossy(&body).into_owned())
    }

    fn read_reply(&mut self, id: i32) -> Result<Packet> {
        loop {
            let packet = self.read()?;
            if packet.id == -1 {
                bail!("the server no longer considers this connection authenticated");
            }
            if packet.id == id {
                return Ok(packet);
            }
        }
    }

    fn send(&mut self, kind: i32, body: &str) -> Result<i32> {
        let id = self.next_id;
        // Stay positive: -1 is how the server signals a failed login.
        self.next_id = if id == i32::MAX { 1 } else { id + 1 };

        let mut packet = Vec::with_capacity(body.len() + 14);
        packet.extend((body.len() as i32 + 10).to_le_bytes());
        packet.extend(id.to_le_bytes());
        packet.extend(kind.to_le_bytes());
        packet.extend(body.as_bytes());
        packet.extend([0, 0]);
        self.stream()?
            .write_all(&packet)
            .context("connection to the server lost")?;
        Ok(id)
    }

    fn read(&mut self) -> Result<Packet> {
        let stream = self.stream()?;
        let mut length = [0; 4];
        stream
            .read_exact(&mut length)
            .context("connection to the server lost")?;
        let length = i32::from_le_bytes(length) as usize;
        if !(10..=MAX_PACKET_BYTES).contains(&length) {
            bail!("malformed RCON packet (length {length}); is this an RCON port?");
        }
        let mut data = vec![0; length];
        stream
            .read_exact(&mut data)
            .context("connection to the server lost")?;
        Ok(Packet {
            id: i32::from_le_bytes(data[0..4].try_into().unwrap()),
            kind: i32::from_le_bytes(data[4..8].try_into().unwrap()),
            body: data[8..length - 2].to_vec(),
        })
    }

    fn stream(&mut self) -> Result<&mut TcpStream> {
        self.stream.as_mut().ok_or_else(|| anyhow!("not connected"))
    }
}
