use crate::{
    model::*,
    store::Store,
    wire::{self, Assembler, Channel, Envelope, Hello, Packet, Transfer},
};
use anyhow::{bail, ensure, Context, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use parking_lot::Mutex;
use rand::Rng;
use serde::Serialize;
use snow::Builder;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, oneshot},
};
use tokio_util::sync::CancellationToken;

const SERVICE: &str = "_clipvault._tcp.local.";

#[derive(Clone, Debug, Serialize)]
pub struct NearbyHost {
    pub id: String,
    pub name: String,
    pub address: String,
    pub code: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Pairing {
    pub id: String,
    pub device_name: String,
    pub verification: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub paused: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub phase: String,
    pub role: Option<String>,
    pub address: Option<String>,
    pub join_code: Option<String>,
    pub code_expires_at: Option<f64>,
    pub paused: bool,
    pub room_paused: bool,
    pub peers: Vec<Peer>,
    pub pairings: Vec<Pairing>,
    pub error: Option<String>,
}
impl Default for Status {
    fn default() -> Self {
        Self {
            phase: "offline".into(),
            role: None,
            address: None,
            join_code: None,
            code_expires_at: None,
            paused: false,
            room_paused: false,
            peers: vec![],
            pairings: vec![],
            error: None,
        }
    }
}
#[derive(Clone, Debug)]
pub enum Event {
    Status(Status),
    Remote(Arc<Transfer>),
    Discovery,
    Error(String),
}

struct Connection {
    peer: Peer,
    sender: mpsc::Sender<Outgoing>,
    token: CancellationToken,
}
enum Outgoing {
    Packet(Packet),
    Transfer(Arc<Transfer>),
}
struct Pending {
    info: Pairing,
    answer: oneshot::Sender<bool>,
}
struct State {
    status: Status,
    cancel: CancellationToken,
    session: String,
    sequence: u64,
    connections: HashMap<String, Connection>,
    pending: HashMap<String, Pending>,
    seen: HashSet<String>,
    seen_order: VecDeque<String>,
    advertisement: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            status: Status::default(),
            cancel: CancellationToken::new(),
            session: uuid::Uuid::new_v4().to_string(),
            sequence: 0,
            connections: HashMap::new(),
            pending: HashMap::new(),
            seen: HashSet::new(),
            seen_order: VecDeque::new(),
            advertisement: None,
        }
    }
}

pub struct SyncService {
    store: Arc<Store>,
    private_key: Vec<u8>,
    device: TrustedDevice,
    state: Mutex<State>,
    events: broadcast::Sender<Event>,
    mdns: Option<ServiceDaemon>,
    discovered: Mutex<HashMap<String, NearbyHost>>,
}
impl SyncService {
    pub fn generate_key() -> Result<Vec<u8>> {
        Ok(Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse()?)
            .generate_keypair()?
            .private)
    }
    pub fn new(store: Arc<Store>, private_key: Vec<u8>, discovery: bool) -> Result<Arc<Self>> {
        // Derive the public key with the same Curve25519 implementation used by Noise.
        let resolver = snow::resolvers::DefaultResolver;
        use snow::resolvers::CryptoResolver;
        let mut dh = resolver
            .resolve_dh(&snow::params::DHChoice::Curve25519)
            .context("Curve25519 unavailable")?;
        ensure!(private_key.len() == 32, "Invalid identity key");
        dh.set(&private_key);
        let key = dh.pubkey().to_vec();
        let settings = store.settings()?;
        let (events, _) = broadcast::channel(32);
        let service = Arc::new(Self {
            store,
            private_key,
            device: TrustedDevice {
                id: digest(&key),
                name: settings.device_name,
                public_key: hex::encode(key),
            },
            state: Mutex::new(State::default()),
            events,
            mdns: if discovery {
                ServiceDaemon::new().ok()
            } else {
                None
            },
            discovered: Mutex::new(HashMap::new()),
        });
        if let Some(mdns) = &service.mdns {
            if let Ok(receiver) = mdns.browse(SERVICE) {
                let weak = Arc::downgrade(&service);
                tokio::spawn(async move {
                    while let Ok(event) = receiver.recv_async().await {
                        let Some(service) = weak.upgrade() else { break };
                        match event {
                            ServiceEvent::ServiceResolved(info) => {
                                let id = info.get_property_val_str("id").unwrap_or_default();
                                if id == service.device.id
                                    || id.len() != 64
                                    || info.get_property_val_str("version") != Some("1")
                                {
                                    continue;
                                }
                                if let Some(ip) = info.get_addresses_v4().into_iter().next() {
                                    let host = NearbyHost {
                                        id: id.into(),
                                        name: info
                                            .get_property_val_str("name")
                                            .unwrap_or("ClipVault")
                                            .chars()
                                            .take(128)
                                            .collect(),
                                        address: SocketAddr::new(ip.into(), info.get_port())
                                            .to_string(),
                                        code: info
                                            .get_property_val_str("code")
                                            .unwrap_or("")
                                            .into(),
                                    };
                                    service
                                        .discovered
                                        .lock()
                                        .insert(info.get_fullname().into(), host);
                                    let _ = service.events.send(Event::Discovery);
                                }
                            }
                            ServiceEvent::ServiceRemoved(_, name) => {
                                service.discovered.lock().remove(&name);
                                let _ = service.events.send(Event::Discovery);
                            }
                            _ => {}
                        }
                    }
                });
            }
        }
        Ok(service)
    }
    pub fn device_id(&self) -> &str {
        &self.device.id
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
    pub fn status(&self) -> Status {
        let state = self.state.lock();
        let mut status = state.status.clone();
        status.peers = state.connections.values().map(|c| c.peer.clone()).collect();
        status.pairings = state.pending.values().map(|p| p.info.clone()).collect();
        if status
            .code_expires_at
            .is_some_and(|expires| expires < now())
        {
            status.join_code = None;
        }
        status
    }
    fn emit(&self) {
        let _ = self.events.send(Event::Status(self.status()));
    }
    fn error(&self, error: impl std::fmt::Display) {
        let message = error.to_string();
        self.state.lock().status.error = Some(message.clone());
        let _ = self.events.send(Event::Error(message));
        self.emit();
    }
    pub fn nearby(&self) -> Vec<NearbyHost> {
        self.discovered.lock().values().cloned().collect()
    }
    pub fn interfaces() -> Vec<(String, String)> {
        local_ip_address::list_afinet_netifas()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, ip)| ip.is_ipv4() && !ip.is_loopback() && !ip.is_unspecified())
            .map(|(name, ip)| (name, ip.to_string()))
            .collect()
    }
    pub fn stop(&self, persist: bool) -> Result<()> {
        let mut state = self.state.lock();
        state.cancel.cancel();
        if let (Some(mdns), Some(name)) = (&self.mdns, state.advertisement.take()) {
            let _ = mdns.unregister(&name);
        }
        *state = State::default();
        drop(state);
        if persist {
            self.store.set("session", &SavedSession::default())?;
        }
        self.emit();
        Ok(())
    }
    pub async fn resume(self: &Arc<Self>) -> Result<()> {
        let saved: SavedSession = self.store.get("session")?.unwrap_or_default();
        match saved.role.as_deref() {
            Some("host") => {
                self.start_host(saved.bind_address, true).await?;
            }
            Some("guest") => {
                if let (Some(id), Some(address)) = (saved.host_id, saved.address) {
                    self.begin_join(address, None, Some(id), saved.paused)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub async fn start_host(self: &Arc<Self>, bind: Option<String>, resuming: bool) -> Result<()> {
        let old: SavedSession = self.store.get("session")?.unwrap_or_default();
        let ip: IpAddr = match bind.as_deref() {
            Some(value) => value.parse()?,
            None => local_ip_address::local_ip().context("No local network connection")?,
        };
        ensure!(
            !ip.is_unspecified() && !ip.is_multicast(),
            "Select a local network interface"
        );
        let port = if resuming {
            old.address
                .as_ref()
                .and_then(|s| s.parse::<SocketAddr>().ok())
                .map_or(0, |a| a.port())
        } else {
            0
        };
        let listener = TcpListener::bind(SocketAddr::new(ip, port))
            .await
            .context("Could not start the local server; select Host Server to use a new port")?;
        let address = listener.local_addr()?;
        self.stop(false)?;
        let cancel = {
            let mut state = self.state.lock();
            state.status = Status {
                phase: "hosting".into(),
                role: Some("host".into()),
                address: Some(address.to_string()),
                paused: resuming && old.paused,
                room_paused: resuming && old.paused,
                ..Status::default()
            };
            state.cancel.clone()
        };
        self.store.set(
            "session",
            &SavedSession {
                role: Some("host".into()),
                address: Some(address.to_string()),
                bind_address: Some(ip.to_string()),
                paused: resuming && old.paused,
                ..SavedSession::default()
            },
        )?;
        if !resuming {
            self.rotate_code()?;
        } else {
            self.advertise_or_report();
        }
        self.emit();
        let service = self.clone();
        tokio::spawn(async move {
            let mut attempts: HashMap<IpAddr, VecDeque<Instant>> = HashMap::new();
            let permits = Arc::new(tokio::sync::Semaphore::new(16));
            let mut expiry = tokio::time::interval(Duration::from_secs(1));
            loop {
                tokio::select! {
                    _=cancel.cancelled()=>break,
                    _=expiry.tick()=>{service.expire_code();},
                    incoming=listener.accept()=>match incoming {
                        Ok((socket,address))=>{
                            let now=Instant::now();attempts.retain(|_,v|v.back().is_some_and(|t|now.duration_since(*t)<Duration::from_secs(60)));
                            if attempts.len()>256&&!attempts.contains_key(&address.ip()) {continue;}
                            let times=attempts.entry(address.ip()).or_default();while times.front().is_some_and(|t|now.duration_since(*t)>Duration::from_secs(60)){times.pop_front();}
                            if times.len()>=5{continue;} times.push_back(now);
                            let Ok(permit)=permits.clone().try_acquire_owned() else {continue;};
                            let service=service.clone();let child=cancel.child_token();
                            tokio::spawn(async move {let _permit=permit; if let Err(error)=service.connection(socket,None,None,child.clone()).await {if !child.is_cancelled(){service.error(error);}}});
                        }
                        Err(error)=>{service.error(error);break;}
                    }
                }
            }
        });
        Ok(())
    }
    pub fn rotate_code(&self) -> Result<()> {
        let mut state = self.state.lock();
        ensure!(
            state.status.role.as_deref() == Some("host"),
            "Start a server first"
        );
        state.status.join_code = Some(format!("{:06}", rand::thread_rng().gen_range(0..1_000_000)));
        state.status.code_expires_at = Some(now() + 300.0);
        drop(state);
        self.advertise_or_report();
        self.emit();
        Ok(())
    }
    fn expire_code(&self) {
        let expired = {
            let mut state = self.state.lock();
            if state.status.code_expires_at.is_some_and(|t| t < now())
                && state.status.join_code.is_some()
            {
                state.status.join_code = None;
                true
            } else {
                false
            }
        };
        if expired {
            self.advertise_or_report();
            self.emit();
        }
    }
    fn advertise_or_report(&self) {
        if let Err(error) = self.advertise() {
            self.error(format!(
                "LAN discovery is unavailable. Join with the host IP and port: {error}"
            ));
        }
    }
    fn advertise(&self) -> Result<()> {
        let Some(mdns) = &self.mdns else {
            return Ok(());
        };
        let status = self.status();
        let Some(address) = status.address else {
            return Ok(());
        };
        let address: SocketAddr = address.parse()?;
        let properties = HashMap::from([
            ("id".into(), self.device.id.clone()),
            ("name".into(), self.store.settings()?.device_name),
            ("version".into(), "1".into()),
            ("code".into(), status.join_code.unwrap_or_default()),
        ]);
        let instance = format!("ClipVault-{}", &self.device.id[..12]);
        let info = ServiceInfo::new(
            SERVICE,
            &instance,
            &format!("{instance}.local."),
            address.ip(),
            address.port(),
            properties,
        )?;
        self.state.lock().advertisement = Some(info.get_fullname().into());
        mdns.register(info)?;
        Ok(())
    }
    pub fn join(self: &Arc<Self>, code: String, address: Option<String>) -> Result<()> {
        ensure!(
            code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit()),
            "Enter the six-digit join code"
        );
        let (address, expected) = if let Some(address) = address.filter(|s| !s.trim().is_empty()) {
            (address, None)
        } else {
            let matches: Vec<_> = self
                .nearby()
                .into_iter()
                .filter(|h| h.code == code)
                .collect();
            ensure!(
                matches.len() == 1,
                if matches.is_empty() {
                    "Server not found. Check the network or enter its IP and port."
                } else {
                    "Multiple servers have this code. Enter the host's IP and port."
                }
            );
            (matches[0].address.clone(), Some(matches[0].id.clone()))
        };
        self.begin_join(address, Some(code), expected, false)
    }
    fn begin_join(
        self: &Arc<Self>,
        address: String,
        code: Option<String>,
        expected: Option<String>,
        paused: bool,
    ) -> Result<()> {
        address
            .parse::<SocketAddr>()
            .context("Enter an IP address and port, such as 192.168.1.5:43210")?;
        self.stop(false)?;
        if code.is_some() {
            self.store.set("session", &SavedSession::default())?;
        }
        let cancel = {
            let mut state = self.state.lock();
            state.status = Status {
                phase: "connecting".into(),
                role: Some("guest".into()),
                address: Some(address.clone()),
                paused,
                ..Status::default()
            };
            state.cancel.clone()
        };
        self.emit();
        let service = self.clone();
        tokio::spawn(async move {
            let mut delay = 1;
            let mut expected = expected;
            let mut code = code;
            loop {
                if cancel.is_cancelled() {
                    break;
                }
                let current = expected
                    .as_ref()
                    .and_then(|id| service.nearby().into_iter().find(|h| &h.id == id))
                    .map(|h| h.address)
                    .unwrap_or_else(|| address.clone());
                {
                    let mut state = service.state.lock();
                    state.status.address = Some(current.clone());
                }
                let attempt = async {
                    let socket =
                        tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&current))
                            .await
                            .context("Server did not respond")??;
                    service
                        .connection(
                            socket,
                            Some(Hello {
                                version: 1,
                                join_code: code.clone(),
                            }),
                            expected.clone(),
                            cancel.child_token(),
                        )
                        .await
                };
                let result = tokio::select! {_=cancel.cancelled()=>break,result=attempt=>result};
                if cancel.is_cancelled() {
                    break;
                }
                let saved: SavedSession = service
                    .store
                    .get("session")
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                if saved.role.as_deref() == Some("guest") && saved.host_id.is_some() {
                    expected = saved.host_id;
                    code = None;
                } else {
                    if let Err(error) = result {
                        service.error(error);
                    }
                    service.state.lock().status.phase = "offline".into();
                    service.emit();
                    break;
                }
                {
                    service.state.lock().status.phase = "reconnecting".into();
                }
                if let Err(error) = result {
                    service.error(error);
                }
                service.emit();
                tokio::select! {_=cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(delay))=>{}}
                delay = (delay * 2).min(30);
            }
        });
        Ok(())
    }
    pub fn confirm(&self, id: &str, accepted: bool) -> Result<()> {
        let pending = self
            .state
            .lock()
            .pending
            .remove(id)
            .context("This pairing request has expired")?;
        let _ = pending.answer.send(accepted);
        self.emit();
        Ok(())
    }
    async fn authenticate(
        &self,
        channel: &mut Channel,
        hello: &Hello,
        host: bool,
        expected: Option<&str>,
    ) -> Result<TrustedDevice> {
        let mut own = self.device.clone();
        own.name = self.store.settings()?.device_name;
        channel
            .writer
            .packet(&Packet::Identity { device: own })
            .await?;
        let peer = match channel.reader.packet().await? {
            Packet::Identity { device } => device,
            _ => bail!("Expected peer identity"),
        };
        ensure!(
            peer.id == digest(&channel.remote_key)
                && peer.public_key == hex::encode(&channel.remote_key),
            "Peer identity does not match its encryption key"
        );
        ensure!(
            peer.id != self.device.id && !peer.name.is_empty() && peer.name.len() <= 128,
            "Invalid peer identity"
        );
        if let Some(expected) = expected {
            ensure!(
                peer.id == expected,
                "The host identity changed. Forget the old device and pair again."
            );
        }
        let known = self
            .store
            .trusted()?
            .iter()
            .any(|d| d.id == peer.id && d.public_key == peer.public_key);
        let valid_code = {
            let state = self.state.lock();
            hello.join_code.is_some()
                && hello.join_code == state.status.join_code
                && state.status.code_expires_at.is_some_and(|t| t >= now())
        };
        ensure!(
            !host || known || valid_code,
            "Incorrect or expired join code"
        );
        channel.writer.packet(&Packet::Trust { known }).await?;
        let remote_known = match channel.reader.packet().await? {
            Packet::Trust { known } => known,
            _ => bail!("Expected trust state"),
        };
        if !known || !remote_known {
            ensure!(
                !host || valid_code,
                "Open a new pairing code on the host to reconnect this device"
            );
            ensure!(
                host || hello.join_code.is_some(),
                "Pairing was removed on the host. Join again using a new code."
            );
            let id = uuid::Uuid::new_v4().to_string();
            let (send, receive) = oneshot::channel();
            {
                let mut state = self.state.lock();
                ensure!(state.pending.len() < 4, "Too many pending pairing requests");
                state.pending.insert(
                    id.clone(),
                    Pending {
                        info: Pairing {
                            id: id.clone(),
                            device_name: peer.name.clone(),
                            verification: channel.verification.clone(),
                        },
                        answer: send,
                    },
                );
            }
            self.emit();
            let result = tokio::time::timeout(Duration::from_secs(120), async {
                tokio::try_join!(
                    async {
                        let accepted = receive.await.unwrap_or(false);
                        channel.writer.packet(&Packet::Confirm { accepted }).await?;
                        ensure!(accepted, "Pairing rejected");
                        Ok::<_, anyhow::Error>(())
                    },
                    async {
                        match channel.reader.packet().await? {
                            Packet::Confirm { accepted: true } => Ok(()),
                            _ => bail!("The other device rejected pairing"),
                        }
                    }
                )?;
                Ok::<_, anyhow::Error>(())
            })
            .await;
            self.state.lock().pending.remove(&id);
            self.emit();
            result.context("Pairing expired")??;
            self.store.trust(&peer)?;
        }
        Ok(peer)
    }
    async fn connection(
        self: &Arc<Self>,
        socket: TcpStream,
        hello: Option<Hello>,
        expected: Option<String>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let host = hello.is_none();
        let (mut channel, hello) = tokio::select! {_=cancel.cancelled()=>return Ok(()),result=wire::handshake(socket,&self.private_key,hello)=>result?};
        let peer = tokio::select! {_=cancel.cancelled()=>return Ok(()),result=tokio::time::timeout(Duration::from_secs(150),self.authenticate(&mut channel,&hello,host,expected.as_deref()))=>result.context("Authentication timed out")??};
        if cancel.is_cancelled() {
            return Ok(());
        }
        if host {
            let (session_id, room_paused) = {
                let state = self.state.lock();
                (state.session.clone(), state.status.paused)
            };
            channel
                .writer
                .packet(&Packet::Welcome {
                    session_id,
                    room_paused,
                })
                .await?;
        } else {
            let (session_id, room_paused) = match channel.reader.packet().await? {
                Packet::Welcome {
                    session_id,
                    room_paused,
                } => (session_id, room_paused),
                _ => bail!("Expected server session"),
            };
            uuid::Uuid::parse_str(&session_id)?;
            {
                let mut state = self.state.lock();
                state.session = session_id;
                state.sequence = 0;
                state.seen.clear();
                state.seen_order.clear();
                state.status.room_paused = room_paused;
            }
            let status = self.status();
            self.store.set(
                "session",
                &SavedSession {
                    role: Some("guest".into()),
                    host_id: Some(peer.id.clone()),
                    address: status.address,
                    paused: status.paused,
                    ..SavedSession::default()
                },
            )?;
        }
        let (sender, mut receiver) = mpsc::channel(32);
        {
            let mut state = self.state.lock();
            ensure!(state.connections.len() < 16, "Server is full");
            if let Some(old) = state.connections.remove(&peer.id) {
                old.token.cancel();
            }
            state.connections.insert(
                peer.id.clone(),
                Connection {
                    peer: Peer {
                        id: peer.id.clone(),
                        name: peer.name.clone(),
                        paused: false,
                    },
                    sender,
                    token: cancel.clone(),
                },
            );
            state.status.phase = if host { "hosting" } else { "connected" }.into();
            state.status.error = None;
        }
        self.emit();
        if !host {
            channel
                .writer
                .packet(&Packet::Pause {
                    paused: self.status().paused,
                })
                .await?;
        }
        let mut writer = channel.writer;
        let mut reader = channel.reader;
        let write = async {
            let mut ping = tokio::time::interval(Duration::from_secs(10));
            loop {
                tokio::select! {
                    message=receiver.recv()=>match message {Some(Outgoing::Packet(packet))=>writer.packet(&packet).await?,Some(Outgoing::Transfer(transfer))=>writer.transfer(&transfer).await?,None=>return Ok::<(),anyhow::Error>(())},
                    _=ping.tick()=>writer.packet(&Packet::Ping).await?,
                }
            }
        };
        let read = async {
            let mut assembler = Assembler::default();
            loop {
                let packet = tokio::time::timeout(Duration::from_secs(45), reader.packet())
                    .await
                    .context("Peer connection timed out")??;
                match packet {
                    Packet::Ping => {}
                    Packet::Pause { paused } if host => {
                        if let Some(c) = self.state.lock().connections.get_mut(&peer.id) {
                            c.peer.paused = paused;
                        }
                        self.emit();
                    }
                    Packet::RoomPause { paused } if !host => {
                        self.state.lock().status.room_paused = paused;
                        self.emit();
                    }
                    Packet::Error { message } => {
                        bail!("{}", message.chars().take(300).collect::<String>())
                    }
                    packet @ (Packet::Begin { .. } | Packet::Chunk { .. } | Packet::End { .. }) => {
                        if let Some(mut transfer) = assembler.accept(packet)? {
                            if host {
                                transfer.envelope.origin = peer.clone();
                                self.accept_host(transfer, true, Some(&peer.id))?;
                            } else {
                                self.accept_remote(transfer)?;
                            }
                        }
                    }
                    _ => bail!("Unexpected message after authentication"),
                }
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        };
        let result =
            tokio::select! {_=cancel.cancelled()=>Ok(()),result=write=>result,result=read=>result};
        cancel.cancel();
        {
            let mut state = self.state.lock();
            if state
                .connections
                .get(&peer.id)
                .is_some_and(|c| c.token.is_cancelled())
            {
                state.connections.remove(&peer.id);
            }
        }
        self.emit();
        result
    }
    fn remember_event(state: &mut State, id: &str) -> bool {
        if !state.seen.insert(id.into()) {
            return false;
        }
        state.seen_order.push_back(id.into());
        if state.seen_order.len() > 4096 {
            if let Some(id) = state.seen_order.pop_front() {
                state.seen.remove(&id);
            }
        }
        true
    }
    fn accept_host(&self, mut transfer: Transfer, apply: bool, sender: Option<&str>) -> Result<()> {
        let transfer = {
            let mut state = self.state.lock();
            if state.status.paused
                || state.status.role.as_deref() != Some("host")
                || sender.is_some_and(|id| state.connections.get(id).is_none_or(|c| c.peer.paused))
            {
                return Ok(());
            }
            if !Self::remember_event(&mut state, &transfer.envelope.event_id) {
                return Ok(());
            }
            ensure!(
                transfer.envelope.session_id == state.session,
                "Clipboard belongs to another session"
            );
            state.sequence = state
                .sequence
                .checked_add(1)
                .context("Session sequence exhausted")?;
            transfer.envelope.sequence = state.sequence;
            transfer.envelope.session_id = state.session.clone();
            let transfer = Arc::new(transfer);
            for c in state.connections.values() {
                if !c.peer.paused
                    && c.sender
                        .try_send(Outgoing::Transfer(transfer.clone()))
                        .is_err()
                {
                    c.token.cancel();
                }
            }
            transfer
        };
        if apply {
            let _ = self.events.send(Event::Remote(transfer));
        }
        Ok(())
    }
    fn accept_remote(&self, transfer: Transfer) -> Result<()> {
        {
            let mut state = self.state.lock();
            ensure!(
                transfer.envelope.session_id == state.session,
                "Clipboard belongs to another session"
            );
            if transfer.envelope.sequence <= state.sequence {
                return Ok(());
            }
            state.sequence = transfer.envelope.sequence;
            if state.status.paused
                || state.status.room_paused
                || !Self::remember_event(&mut state, &transfer.envelope.event_id)
            {
                return Ok(());
            }
        }
        let _ = self.events.send(Event::Remote(Arc::new(transfer)));
        Ok(())
    }
    pub fn publish(&self, payload: Payload) -> Result<()> {
        payload.validate()?;
        let status = self.status();
        if status.paused || status.room_paused {
            return Ok(());
        }
        let transfer = Transfer {
            envelope: Envelope {
                version: PROTOCOL_VERSION,
                event_id: uuid::Uuid::new_v4().to_string(),
                origin: TrustedDevice {
                    name: self.store.settings()?.device_name,
                    ..self.device.clone()
                },
                session_id: self.state.lock().session.clone(),
                sequence: 0,
                kind: payload.kind,
                size: payload.bytes.len(),
                hash: payload.hash(),
            },
            payload,
        };
        if status.role.as_deref() == Some("host") {
            self.accept_host(transfer, false, None)?;
        } else {
            let state = self.state.lock();
            for c in state.connections.values() {
                if c.sender
                    .try_send(Outgoing::Transfer(Arc::new(transfer.clone())))
                    .is_err()
                {
                    c.token.cancel();
                }
            }
        }
        Ok(())
    }
    pub fn should_apply(&self, transfer: &Transfer) -> bool {
        let state = self.state.lock();
        !state.status.paused
            && !state.status.room_paused
            && state.status.role.is_some()
            && state.session == transfer.envelope.session_id
            && transfer.envelope.sequence == state.sequence
    }
    pub fn should_record(&self, transfer: &Transfer) -> bool {
        let state = self.state.lock();
        !state.status.paused
            && !state.status.room_paused
            && state.status.role.is_some()
            && state.session == transfer.envelope.session_id
            && transfer.envelope.sequence <= state.sequence
    }
    pub fn pause(&self, paused: bool) -> Result<()> {
        {
            let mut state = self.state.lock();
            state.status.paused = paused;
            let host = state.status.role.as_deref() == Some("host");
            if host {
                state.status.room_paused = paused;
            }
            for c in state.connections.values() {
                let packet = if host {
                    Packet::RoomPause { paused }
                } else {
                    Packet::Pause { paused }
                };
                if c.sender.try_send(Outgoing::Packet(packet)).is_err() {
                    c.token.cancel();
                }
            }
        }
        let mut saved: SavedSession = self.store.get("session")?.unwrap_or_default();
        saved.paused = paused;
        self.store.set("session", &saved)?;
        self.emit();
        Ok(())
    }
    pub fn forget(&self, id: &str) -> Result<()> {
        self.store.forget(id)?;
        if let Some(c) = self.state.lock().connections.remove(id) {
            c.token.cancel();
        }
        let saved: SavedSession = self.store.get("session")?.unwrap_or_default();
        if saved.host_id.as_deref() == Some(id) {
            self.stop(true)?;
        }
        self.emit();
        Ok(())
    }
}
impl Drop for SyncService {
    fn drop(&mut self) {
        self.state.get_mut().cancel.cancel();
        if let Some(mdns) = &self.mdns {
            let _ = mdns.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn rejected_connection(
        expired: bool,
        changed_identity: bool,
    ) -> Result<(String, String)> {
        let a = tempfile::tempdir()?;
        let b = tempfile::tempdir()?;
        let host = SyncService::new(
            Arc::new(Store::open(a.path())?),
            SyncService::generate_key()?,
            false,
        )?;
        let guest = SyncService::new(
            Arc::new(Store::open(b.path())?),
            SyncService::generate_key()?,
            false,
        )?;
        {
            let mut state = host.state.lock();
            state.status.role = Some("host".into());
            state.status.join_code = Some("123456".into());
            state.status.code_expires_at = Some(now() + if expired { -1.0 } else { 300.0 });
        }
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let host_task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await?;
            let (mut channel, hello) = wire::handshake(socket, &host.private_key, None).await?;
            let result = host.authenticate(&mut channel, &hello, true, None).await;
            assert!(host.store.trusted()?.is_empty());
            Ok::<_, anyhow::Error>(result.unwrap_err().to_string())
        });
        let (mut channel, hello) = wire::handshake(
            TcpStream::connect(address).await?,
            &guest.private_key,
            Some(Hello {
                version: 1,
                join_code: if expired || changed_identity {
                    Some("123456".into())
                } else {
                    None
                },
            }),
        )
        .await?;
        let old_id = "0".repeat(64);
        let error = guest
            .authenticate(
                &mut channel,
                &hello,
                false,
                changed_identity.then_some(old_id.as_str()),
            )
            .await
            .unwrap_err();
        drop(channel);
        assert!(guest.store.trusted()?.is_empty());
        Ok((host_task.await??, error.to_string()))
    }
    #[tokio::test]
    async fn expired_code_never_starts_pairing() -> Result<()> {
        assert!(rejected_connection(true, false)
            .await?
            .0
            .contains("expired join code"));
        Ok(())
    }
    #[tokio::test]
    async fn changed_host_key_requires_explicit_repairing() -> Result<()> {
        assert!(rejected_connection(false, true)
            .await?
            .1
            .contains("identity changed"));
        Ok(())
    }
    #[tokio::test]
    async fn unknown_device_without_code_is_rejected() -> Result<()> {
        assert!(rejected_connection(false, false)
            .await?
            .0
            .contains("join code"));
        Ok(())
    }
}
