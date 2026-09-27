use anyhow::{Context, Result};
use clipvault_core::{
    model::*,
    store::Store,
    sync::{Event, SyncService},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;

struct Device {
    _dir: tempfile::TempDir,
    store: Arc<Store>,
    app: Arc<SyncService>,
}
impl Device {
    fn new(name: &str) -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let store = Arc::new(Store::open(dir.path())?);
        store.save_settings(&Settings {
            device_name: name.into(),
            ..Settings::default()
        })?;
        let app = SyncService::new(store.clone(), SyncService::generate_key()?, false)?;
        Ok(Self {
            _dir: dir,
            store,
            app,
        })
    }
}
impl Drop for Device {
    fn drop(&mut self) {
        let _ = self.app.stop(false);
    }
}
async fn until(mut predicate: impl FnMut() -> bool) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(8), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("Condition did not become true")?;
    Ok(())
}
async fn pair(host: &Device, guest: &Device) -> Result<()> {
    guest.app.join(
        host.app.status().join_code.context("No code")?,
        host.app.status().address,
    )?;
    until(|| !host.app.status().pairings.is_empty() && !guest.app.status().pairings.is_empty())
        .await?;
    let a = host.app.status().pairings[0].clone();
    let b = guest.app.status().pairings[0].clone();
    assert_eq!(a.verification, b.verification);
    host.app.confirm(&a.id, true)?;
    assert!(
        !host
            .store
            .trusted()?
            .iter()
            .any(|d| d.id == guest.app.device_id()),
        "One-sided confirmation must not authorize a device"
    );
    guest.app.confirm(&b.id, true)?;
    until(|| {
        guest.app.status().phase == "connected"
            && host
                .app
                .status()
                .peers
                .iter()
                .any(|p| p.id == guest.app.device_id())
    })
    .await
}
async fn remote(
    events: &mut broadcast::Receiver<Event>,
) -> Result<Arc<clipvault_core::wire::Transfer>> {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if let Event::Remote(t) = events.recv().await? {
                return Ok::<_, anyhow::Error>(t);
            }
        }
    })
    .await
    .context("No clipboard received")?
}
fn drain(events: &mut broadcast::Receiver<Event>) {
    while events.try_recv().is_ok() {}
}
async fn no_remote(events: &mut broadcast::Receiver<Event>) {
    assert!(tokio::time::timeout(Duration::from_millis(160), async {
        loop {
            if let Ok(Event::Remote(_)) = events.recv().await {
                return;
            }
        }
    })
    .await
    .is_err());
}
fn text(value: &str) -> Payload {
    Payload {
        kind: Kind::Text,
        bytes: value.as_bytes().to_vec(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_devices_ordered_sharing_pause_reconnect_and_revocation() -> Result<()> {
    let a = Device::new("Mac")?;
    let b = Device::new("Linux ARM")?;
    let c = Device::new("Linux AMD")?;
    a.app.start_host(Some("127.0.0.1".into()), false).await?;
    pair(&a, &b).await?;
    pair(&a, &c).await?;
    let (mut ar, mut br, mut cr) = (a.app.subscribe(), b.app.subscribe(), c.app.subscribe());
    b.app.publish(text("hello 🌍\nfrom Linux"))?;
    let (ra, rb, rc) = (
        remote(&mut ar).await?,
        remote(&mut br).await?,
        remote(&mut cr).await?,
    );
    assert_eq!(ra.payload, text("hello 🌍\nfrom Linux"));
    assert_eq!(ra.envelope.sequence, rc.envelope.sequence);
    assert_eq!(rb.envelope.origin.name, "Linux ARM");
    no_remote(&mut ar).await; // Receiving doesn't rebroadcast content.
    b.app.pause(true)?;
    until(|| {
        a.app
            .status()
            .peers
            .iter()
            .any(|p| p.id == b.app.device_id() && p.paused)
    })
    .await?;
    drain(&mut br);
    a.app.publish(text("only C"))?;
    assert_eq!(remote(&mut cr).await?.payload, text("only C"));
    no_remote(&mut br).await;
    b.app.pause(false)?;
    until(|| {
        a.app
            .status()
            .peers
            .iter()
            .any(|p| p.id == b.app.device_id() && !p.paused)
    })
    .await?;
    no_remote(&mut br).await;
    a.app.pause(true)?;
    until(|| b.app.status().room_paused && c.app.status().room_paused).await?;
    b.app.publish(text("must not travel"))?;
    no_remote(&mut ar).await;
    a.app.pause(false)?;
    until(|| !b.app.status().room_paused && !c.app.status().room_paused).await?;
    let old_address = a.app.status().address;
    a.app.stop(false)?;
    until(|| b.app.status().phase == "reconnecting").await?;
    b.app.publish(text("offline clipboard"))?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    a.app.resume().await?;
    assert_eq!(a.app.status().address, old_address);
    until(|| b.app.status().phase == "connected" && c.app.status().phase == "connected").await?;
    assert!(a.app.status().pairings.is_empty());
    drain(&mut ar);
    drain(&mut br);
    drain(&mut cr);
    no_remote(&mut ar).await;
    c.app.publish(text("after reconnect"))?;
    assert_eq!(remote(&mut ar).await?.payload, text("after reconnect"));
    remote(&mut br).await?;
    remote(&mut cr).await?;
    a.app.forget(b.app.device_id())?;
    until(|| {
        !a.app
            .status()
            .peers
            .iter()
            .any(|p| p.id == b.app.device_id())
    })
    .await?;
    assert!(!a.store.trusted()?.iter().any(|d| d.id == b.app.device_id()));
    a.app.publish(text("revoked device excluded"))?;
    assert_eq!(
        remote(&mut cr).await?.payload,
        text("revoked device excluded")
    );
    drain(&mut br);
    no_remote(&mut br).await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn image_chunks_and_simultaneous_copies_converge() -> Result<()> {
    let a = Device::new("A")?;
    let b = Device::new("B")?;
    let c = Device::new("C")?;
    a.app.start_host(Some("127.0.0.1".into()), false).await?;
    pair(&a, &b).await?;
    pair(&a, &c).await?;
    let (mut ar, mut br, mut cr) = (a.app.subscribe(), b.app.subscribe(), c.app.subscribe());
    let image = image::RgbaImage::from_fn(240, 240, |x, y| {
        image::Rgba([(x * y) as u8, (x * 19 + y * 71) as u8, (x ^ y) as u8, 255])
    });
    let mut encoded = std::io::Cursor::new(vec![]);
    image.write_to(&mut encoded, image::ImageFormat::Png)?;
    assert!(encoded.get_ref().len() > 32 * 1024);
    let png = Payload {
        kind: Kind::Image,
        bytes: encoded.into_inner(),
    };
    b.app.publish(png.clone())?;
    assert_eq!(remote(&mut ar).await?.payload, png);
    assert_eq!(remote(&mut cr).await?.payload, png);
    remote(&mut br).await?;
    b.app.publish(text("B"))?;
    c.app.publish(text("C"))?;
    let b1 = remote(&mut br).await?;
    let b2 = remote(&mut br).await?;
    let c1 = remote(&mut cr).await?;
    let c2 = remote(&mut cr).await?;
    assert!(b2.envelope.sequence > b1.envelope.sequence);
    assert_eq!(b1.envelope.sequence, c1.envelope.sequence);
    assert_eq!(b2.payload, c2.payload);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_code_and_rejected_pairing_never_authorize() -> Result<()> {
    let a = Device::new("A")?;
    let b = Device::new("B")?;
    a.app.start_host(Some("127.0.0.1".into()), false).await?;
    let wrong = if a.app.status().join_code.as_deref() == Some("000000") {
        "111111"
    } else {
        "000000"
    };
    b.app.join(wrong.into(), a.app.status().address)?;
    until(|| b.app.status().phase == "offline").await?;
    assert!(a.store.trusted()?.is_empty());
    assert!(a.app.status().peers.is_empty());
    b.app
        .join(a.app.status().join_code.unwrap(), a.app.status().address)?;
    until(|| !a.app.status().pairings.is_empty() && !b.app.status().pairings.is_empty()).await?;
    a.app.confirm(&a.app.status().pairings[0].id, false)?;
    until(|| b.app.status().phase == "offline").await?;
    assert!(a.store.trusted()?.is_empty());
    assert!(b.store.trusted()?.is_empty());
    Ok(())
}
