use crate::model::*;
use anyhow::{bail, ensure, Context, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use snow::{Builder, TransportState};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
};

const FRAME_LIMIT: usize = 65_535;
pub const CHUNK_SIZE: usize = 32 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hello {
    pub version: u32,
    pub join_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u32,
    pub event_id: String,
    pub origin: TrustedDevice,
    pub session_id: String,
    pub sequence: u64,
    pub kind: Kind,
    pub size: usize,
    pub hash: String,
}
#[derive(Clone, Debug)]
pub struct Transfer {
    pub envelope: Envelope,
    pub payload: Payload,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Packet {
    Identity {
        device: TrustedDevice,
    },
    Trust {
        known: bool,
    },
    Confirm {
        accepted: bool,
    },
    Welcome {
        session_id: String,
        room_paused: bool,
    },
    Begin {
        envelope: Envelope,
    },
    Chunk {
        event_id: String,
        #[serde(with = "base64_bytes")]
        bytes: Vec<u8>,
    },
    End {
        event_id: String,
    },
    Pause {
        paused: bool,
    },
    RoomPause {
        paused: bool,
    },
    Ping,
    Error {
        message: String,
    },
}

pub struct Reader {
    input: OwnedReadHalf,
    crypto: Arc<Mutex<TransportState>>,
}
pub struct Writer {
    output: OwnedWriteHalf,
    crypto: Arc<Mutex<TransportState>>,
}
pub struct Channel {
    pub reader: Reader,
    pub writer: Writer,
    pub remote_key: Vec<u8>,
    pub verification: String,
}

async fn read_frame<R: AsyncRead + Unpin>(input: &mut R, max: usize) -> Result<Vec<u8>> {
    let length = input.read_u32().await? as usize;
    ensure!(length > 0 && length <= max, "Invalid frame size");
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes).await?;
    Ok(bytes)
}
async fn write_frame<W: AsyncWrite + Unpin>(output: &mut W, bytes: &[u8]) -> Result<()> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= FRAME_LIMIT,
        "Frame exceeds protocol limit"
    );
    output.write_u32(bytes.len() as u32).await?;
    output.write_all(bytes).await?;
    output.flush().await?;
    Ok(())
}

pub async fn handshake(
    mut socket: TcpStream,
    private_key: &[u8],
    hello: Option<Hello>,
) -> Result<(Channel, Hello)> {
    socket.set_nodelay(true)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        let initiator = hello.is_some();
        let hello_bytes = if let Some(hello) = hello {
            let bytes = serde_json::to_vec(&hello)?;
            write_frame(&mut socket, &bytes).await?;
            bytes
        } else {
            read_frame(&mut socket, 1024).await?
        };
        let hello: Hello = serde_json::from_slice(&hello_bytes)?;
        ensure!(
            hello.version == PROTOCOL_VERSION,
            "Incompatible ClipVault protocol; update both devices"
        );
        if let Some(code) = &hello.join_code {
            ensure!(
                code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()),
                "Invalid join code"
            );
        }
        let mut prologue = b"ClipVault LAN protocol v1\0".to_vec();
        prologue.extend(&hello_bytes);
        let builder = Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse()?)
            .local_private_key(private_key)?
            .prologue(&prologue)?;
        let mut state = if initiator {
            builder.build_initiator()?
        } else {
            builder.build_responder()?
        };
        let mut output = [0u8; 1024];
        if initiator {
            let n = state.write_message(&[], &mut output)?;
            write_frame(&mut socket, &output[..n]).await?;
            let bytes = read_frame(&mut socket, 1024).await?;
            state.read_message(&bytes, &mut output)?;
            let n = state.write_message(&[], &mut output)?;
            write_frame(&mut socket, &output[..n]).await?;
        } else {
            let bytes = read_frame(&mut socket, 1024).await?;
            state.read_message(&bytes, &mut output)?;
            let n = state.write_message(&[], &mut output)?;
            write_frame(&mut socket, &output[..n]).await?;
            let bytes = read_frame(&mut socket, 1024).await?;
            state.read_message(&bytes, &mut output)?;
        }
        let remote_key = state
            .get_remote_static()
            .context("Missing peer identity")?
            .to_vec();
        let mut bound = b"ClipVault verification v1\0".to_vec();
        bound.extend(state.get_handshake_hash());
        let verification = digest(&bound)[..16].to_uppercase();
        let crypto = Arc::new(Mutex::new(state.into_transport_mode()?));
        let (input, output) = socket.into_split();
        Ok((
            Channel {
                reader: Reader {
                    input,
                    crypto: crypto.clone(),
                },
                writer: Writer { output, crypto },
                remote_key,
                verification,
            },
            hello,
        ))
    })
    .await
    .context("Connection handshake timed out")?
}
impl Reader {
    pub async fn packet(&mut self) -> Result<Packet> {
        let encrypted = read_frame(&mut self.input, FRAME_LIMIT).await?;
        let mut plaintext = vec![0; FRAME_LIMIT];
        let n = self
            .crypto
            .lock()
            .read_message(&encrypted, &mut plaintext)
            .context("Invalid encrypted frame")?;
        Ok(serde_json::from_slice(&plaintext[..n])?)
    }
}
impl Writer {
    pub async fn packet(&mut self, packet: &Packet) -> Result<()> {
        let plain = serde_json::to_vec(packet)?;
        ensure!(plain.len() <= FRAME_LIMIT - 16, "Message too large");
        let mut encrypted = vec![0; FRAME_LIMIT];
        let n = self.crypto.lock().write_message(&plain, &mut encrypted)?;
        tokio::time::timeout(
            Duration::from_secs(10),
            write_frame(&mut self.output, &encrypted[..n]),
        )
        .await
        .context("Peer stopped receiving")??;
        Ok(())
    }
    pub async fn transfer(&mut self, transfer: &Transfer) -> Result<()> {
        self.packet(&Packet::Begin {
            envelope: transfer.envelope.clone(),
        })
        .await?;
        for bytes in transfer.payload.bytes.chunks(CHUNK_SIZE) {
            self.packet(&Packet::Chunk {
                event_id: transfer.envelope.event_id.clone(),
                bytes: bytes.to_vec(),
            })
            .await?;
        }
        self.packet(&Packet::End {
            event_id: transfer.envelope.event_id.clone(),
        })
        .await
    }
}

#[derive(Default)]
pub struct Assembler {
    current: Option<(Envelope, Vec<u8>, std::time::Instant)>,
}
impl Assembler {
    pub fn accept(&mut self, packet: Packet) -> Result<Option<Transfer>> {
        if let Some((_, _, start)) = &self.current {
            ensure!(
                start.elapsed() < Duration::from_secs(30),
                "Clipboard transfer timed out"
            );
        }
        match packet {
            Packet::Begin { envelope } => {
                ensure!(
                    envelope.version == PROTOCOL_VERSION,
                    "Incompatible clipboard protocol; update both devices"
                );
                ensure!(self.current.is_none(), "Overlapping clipboard transfers");
                uuid::Uuid::parse_str(&envelope.event_id)?;
                let limit = match envelope.kind {
                    Kind::Text | Kind::Url => MAX_TEXT,
                    Kind::Image => MAX_IMAGE,
                    _ => bail!("Unsupported shared clipboard type"),
                };
                ensure!(
                    envelope.size <= limit && envelope.hash.len() == 64,
                    "Invalid clipboard metadata"
                );
                self.current = Some((envelope, Vec::new(), std::time::Instant::now()));
                Ok(None)
            }
            Packet::Chunk { event_id, bytes } => {
                let (header, content, _) =
                    self.current.as_mut().context("Unexpected content chunk")?;
                ensure!(
                    header.event_id == event_id
                        && bytes.len() <= CHUNK_SIZE
                        && content.len() + bytes.len() <= header.size,
                    "Invalid content chunk"
                );
                content.extend(bytes);
                Ok(None)
            }
            Packet::End { event_id } => {
                let (envelope, bytes, _) =
                    self.current.take().context("Unexpected end of transfer")?;
                ensure!(
                    envelope.event_id == event_id && bytes.len() == envelope.size,
                    "Incomplete clipboard transfer"
                );
                let payload = Payload {
                    kind: envelope.kind,
                    bytes,
                };
                ensure!(payload.hash() == envelope.hash, "Clipboard hash mismatch");
                payload.validate()?;
                Ok(Some(Transfer { envelope, payload }))
            }
            _ => bail!("Unexpected transfer message"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rejects_unbounded_or_empty_frames_before_allocating_payload() -> Result<()> {
        for length in [0, FRAME_LIMIT as u32 + 1, u32::MAX] {
            let (mut send, mut read) = tokio::io::duplex(8);
            send.write_u32(length).await?;
            assert!(read_frame(&mut read, FRAME_LIMIT).await.is_err());
        }
        assert!(serde_json::from_str::<Packet>(r#"{"type":"unknown"}"#).is_err());
        Ok(())
    }
    #[test]
    fn assembler_rejects_oversize_and_bad_hash() {
        let envelope = Envelope {
            version: PROTOCOL_VERSION,
            event_id: uuid::Uuid::new_v4().to_string(),
            origin: TrustedDevice {
                id: "x".into(),
                name: "x".into(),
                public_key: "x".into(),
            },
            session_id: "s".into(),
            sequence: 1,
            kind: Kind::Text,
            size: MAX_TEXT + 1,
            hash: "0".repeat(64),
        };
        assert!(Assembler::default()
            .accept(Packet::Begin {
                envelope: envelope.clone()
            })
            .is_err());
        let mut a = Assembler::default();
        let mut header = envelope;
        header.size = 1;
        a.accept(Packet::Begin {
            envelope: header.clone(),
        })
        .unwrap();
        a.accept(Packet::Chunk {
            event_id: header.event_id.clone(),
            bytes: vec![b'x'],
        })
        .unwrap();
        assert!(a
            .accept(Packet::End {
                event_id: header.event_id
            })
            .is_err());
    }
    #[tokio::test]
    async fn noise_verification_matches_and_encrypts() -> Result<()> {
        let server_key =
            Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse()?).generate_keypair()?;
        let client_key =
            Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse()?).generate_keypair()?;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let (mut channel, _) = handshake(socket, &server_key.private, None).await?;
            assert!(matches!(channel.reader.packet().await?, Packet::Ping));
            channel.writer.packet(&Packet::Ping).await?;
            Ok::<_, anyhow::Error>(channel.verification)
        });
        let (mut channel, _) = handshake(
            TcpStream::connect(address).await?,
            &client_key.private,
            Some(Hello {
                version: 1,
                join_code: Some("123456".into()),
            }),
        )
        .await?;
        channel.writer.packet(&Packet::Ping).await?;
        assert!(matches!(channel.reader.packet().await?, Packet::Ping));
        assert_eq!(channel.verification, task.await??);
        Ok(())
    }
}
