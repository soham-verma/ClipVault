# ClipVault LAN protocol v1

The frontend is bundled inside Tauri. The LAN socket is not a web server and
cannot serve web content or invoke desktop commands.

## Discovery and pairing

Hosts bind to a chosen local interface and advertise `_clipvault._tcp.local.`.
TXT properties contain the protocol version, public device ID, display name,
and an optional six-digit pairing locator. The locator is public discovery
information, not an authentication secret. It expires after five minutes.
Direct IP:port joining works without multicast. A resumed host reuses its port.

Each device has a persistent Curve25519 identity. The device ID is the SHA-256
digest of its public key. The private key lives in Keychain on macOS and a
0600 file inside the 0700 data directory on Linux. Peer public keys are pinned
in SQLite. No certificate authority, relay, or external account is involved.

A length-prefixed JSON Hello (maximum 1024 bytes) specifies version and locator.
Its exact bytes are included in the Noise prologue. The encrypted connection
uses `Noise_XX_25519_ChaChaPoly_BLAKE2s` through `snow`. Handshake messages have a
1024-byte limit and a ten-second deadline. Application records are independently
encrypted Noise messages with a four-byte big-endian length, capped at 65535.

Encrypted Identity and Trust messages follow. A claimed identity must match
the Noise static key. Previously verified peers must match the saved key;
automatic reconnect never accepts a changed host identity. New pairs compare
the first 16 hexadecimal characters of
SHA-256(`ClipVault verification v1\0` || Noise handshake hash).
Both sides explicitly approve, within two minutes, before either permits
clipboard transfer. Rejection or expiry authorizes neither side. Reconnect
requires mutual saved trust; a forgotten peer must pair again with a fresh
locator. Initial untrusted connections are limited to four pending prompts;
the listener permits sixteen connections and five attempts per source IP per
minute. Pairing approval is a human authentication step: never approve without
comparing the full verification value on both devices.

## Live clipboard events

An authenticated host sends Welcome with a random session UUID and room pause
state. Begin/Chunk/End records then transfer one clipboard event at a time:

- Protocol version, UUID event ID, origin identity, session UUID, sequence, kind,
  length, and hash.
- Text/link payloads are UTF-8 (maximum 1 MiB); images are PNG (maximum 20 MiB
  encoded and 32 million decoded pixels).
- Chunks contain up to 32768 raw bytes, base64 encoded inside encrypted JSON.
- The SHA-256 content hash covers kind, a NUL byte, and the payload bytes.
- The complete size, hash, UTF-8/PNG validity, and image dimensions are checked
  before changing a clipboard. Interleaved transfers are rejected.

The host replaces guest-supplied origin metadata with the authenticated
identity and assigns monotonically increasing sequence numbers. All participants
receive that ordering, including the sender. Event IDs suppress duplicates;
platform ownership markers suppress recapturing programmatic writes. Selecting
history explicitly publishes a new event. A bounded outgoing queue disconnects
slow peers instead of retaining unlimited content.

Pause on the host suspends the room; a guest pause excludes that guest. Resume,
join, and reconnect do not replay historical or offline copies. Host exit stops
sharing until it returns. There is no election or historical replication.

Pin/delete actions, application identifiers, file paths, and rich-text blobs
are local. Only the selected shareable representation crosses the network.
Trusted participants and the host can read shared contents by design.

The protocol and application are not independently security-audited.
