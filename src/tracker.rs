use bip_bencode::{BencodeRef, BDecodeOpt, BRefAccess, BDictAccess};
use hyper::Url;
use hyper;
use metainfo::*;
use ring::rand::SystemRandom;
use error::{Error,Result};
use std::io::Read;
use std::time::Duration;
use util::{QueryParameters};

// Client to talk to a tracker
#[derive(Debug)]
pub struct TrackerClient {
    metainfo: MetaInfo,
    peer_id: PeerID,
    url: Url,
    client: hyper::client::Client,
    trackerid: Option<String>,
    user_agent: String,
}

impl TrackerClient {
    pub fn new(metainfo: MetaInfo, peer_id: PeerID) -> Result<TrackerClient> {
        let url = Url::parse(&metainfo.announce)?;
        let mut client = hyper::client::Client::new();
        client.set_read_timeout(Some(Duration::from_secs(10)));
        client.set_write_timeout(Some(Duration::from_secs(10)));
        Ok(TrackerClient {
            metainfo: metainfo,
            peer_id: peer_id,
            url: url,
            client: client,
            trackerid: None,
            user_agent: "Bittles/0.01 rust-lang".to_string(),
        })
    }

    pub fn easy_start(&self) -> Result<TrackerResponse> {
        let req = TrackerRequest {
            info_hash: self.metainfo.info_hash,
            peer_id: self.peer_id,
            port: 6881, // TODO this is not true
            uploaded: 0,
            downloaded: 0,
            left: 0, // TODO
            compact: true,
            no_peer_id: false,
            event: TrackerEvent::Started,
            ip: None,
            numwant: Some(4),
            key: None,
            trackerid: None,
        };
        self.request(&req)
    }

    fn request(&self, req: &TrackerRequest) -> Result<TrackerResponse> {
        let http_req = self.build_req(req)?;
        let mut http_res = http_req.send()?;
        let res = self.parse_res(&mut http_res)?;
        // TODO store trackerid if present.
        Ok(res)
    }

    fn build_req(&self, req: &TrackerRequest) -> Result<hyper::client::RequestBuilder> {
        // TODO set user agent
        let mut qps = QueryParameters::new();
        qps.push("info_hash", req.info_hash);
        qps.push("peer_id", req.peer_id);
        qps.push_num("port", req.port);
        qps.push_num("uploaded", req.uploaded);
        qps.push_num("downloaded", req.downloaded);
        qps.push_num("left", req.left);
        if req.compact {
            qps.push("compact", "1");
        }
        if req.no_peer_id {
            qps.push("no_peer_id", "1");
        }
        if let Some(ev) = req.event.str() {
            qps.push("event", ev);
        }
        if let Some(ref ip) = req.ip {
            qps.push("ip", ip);
        }
        if let Some(numwant) = req.numwant {
            qps.push_num("numwant", numwant);
        }
        if let Some(ref key) = req.key {
            qps.push("key", key);
        }
        if let Some(ref trackerid) = req.trackerid {
            qps.push("trackerid", trackerid);
        }

        let mut url = self.url.clone();
        qps.apply(&mut url);

        let http_req = self.client.get(url)
            .header(hyper::header::UserAgent(self.user_agent.clone()));
        Ok(http_req)
    }

    fn parse_res(&self, http_res: &mut hyper::client::response::Response) -> Result<TrackerResponse> {
        if http_res.status != hyper::status::StatusCode::Ok {
            return Err(Error::new_str(&format!("tracker returned non-200: {}", http_res.status)));
        }
        let mut buf = Vec::new();
        http_res.read_to_end(&mut buf)?;
        let b = BencodeRef::decode(buf.as_slice(), BDecodeOpt::default())?;
        let bd = b.dict().ok_or(Error::new_str("response bencoding not a dict"))?;
        Ok(TrackerResponse {
            failure_reason:  lookup_str(bd, "failure reason".as_bytes())?,
            warning_message: lookup_str(bd, "warning message".as_bytes())?,
            interval:        lookup_i64(bd, "interval".as_bytes())?
                                .ok_or(Error::new_str("missing 'interval'"))?,
            min_interval:    lookup_i64(bd, "min interval".as_bytes())?,
            tracker_id:      lookup_str(bd, "tracker id".as_bytes())?,
            complete:        lookup_i64(bd, "complete".as_bytes())?
                                .ok_or(Error::new_str("missing 'complete'"))?,
            incomplete:      lookup_i64(bd, "incomplete".as_bytes())?
                                .ok_or(Error::new_str("missing 'incomplete'"))?,
            peers:           Vec::new(), // TODO
        })
    }
}

fn lookup_str<'a>(dict: &'a BDictAccess<BencodeRef>, key: &'a [u8]) -> Result<Option<String>> {
    match dict.lookup(key) {
        Some(x) => match x.str() {
            Some(x) => Ok(Some(x.to_owned())),
            None => Err(Error::new_str(&format!("'{:?}' exists but is not a utf8 string", key))),
        },
        None => Ok(None),
    }
}

fn lookup_i64<'a>(dict: &'a BDictAccess<BencodeRef>, key: &'a [u8]) -> Result<Option<i64>> {
    match dict.lookup(key) {
        Some(x) => match x.int() {
            Some(x) => Ok(Some(x)),
            None => Err(Error::new_str(&format!("'{:?}' exists but is not an int", key))),
        },
        None => Ok(None),
    }
}

pub const PEERID_SIZE: usize = 20;
pub type PeerID = [u8; PEERID_SIZE];

pub fn new_peer_id(rand: &SystemRandom) -> Result<PeerID> {
    let mut id = [0; PEERID_SIZE];
    rand.fill(&mut id)?;
    id[0] = '-' as u8;
    id[1] = 'B' as u8;
    id[2] = 'I' as u8;
    id[3] = '0' as u8;
    id[4] = '0' as u8;
    id[5] = '0' as u8;
    id[6] = '1' as u8;
    Ok(id)
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum TrackerEvent {
    Started,
    Stopped,
    Completed,
    Periodical,
}

impl TrackerEvent {
    pub fn str(self) -> Option<String> {
        match self {
            TrackerEvent::Started => Some("started".to_string()),
            TrackerEvent::Stopped => Some("stopped".to_string()),
            TrackerEvent::Completed => Some("completed".to_string()),
            TrackerEvent::Periodical => None,
        }
    }
}

// Parameters used in the client->tracker GET request
pub struct TrackerRequest {
    info_hash: InfoHash, // Hash of the 'info' section of the torrent file
    peer_id: PeerID, // Randomly generated peer id
    port: i64, // Port the client is listening. Typically in [6881-6889]
    uploaded: i64, // Bytes uploaded (since the client sent the 'started' event to the tracker)
    downloaded: i64, // Bytes downloaded (since the client sent the 'started' event to the tracker)
    left: i64, // Bytes left until 100% downloaded
    compact: bool, // Support a compact response
    no_peer_id: bool, // Response can omit peer id field in peers dictionary
    event: TrackerEvent,
    ip: Option<String>, // Outwardly-reachable IP of the client
    numwant: Option<i64>, // Number of peers requested
    key: Option<String>, // Identifier for this client with the tracker
    trackerid: Option<String>, // If a previous announce contained a tracker id, it should be set here
}

// The tracker responds with "text/plain" document consisting of a bencoded dictionary
#[derive(Debug)]
pub struct TrackerResponse {
    failure_reason: Option<String>, // If present, then no other keys may be present. The value is a human-readable error message as to why the request failed (string).
    warning_message: Option<String>, // (new, optional) Similar to failure reason, but the response still gets processed normally. The warning message is shown just like an error.
    interval: i64, // Interval in seconds that the client should wait between sending regular requests to the tracker
    min_interval: Option<i64>, // (optional) Minimum announce interval. If present clients must not reannounce more frequently than this.
    tracker_id: Option<String>, // A string that the client should send back on its next announcements. If absent and a previous announce sent a tracker id, do not discard the old value; keep using it.
    complete: i64, // Number of peers with the entire file (seeders)
    incomplete: i64, // Number of non-seeder peers (leechers)
    peers: Vec<Peer>,
}

// A peer reported by the tracker.
// Contains a peer_id if not in compact form.
#[derive(Debug)]
struct Peer {
    peer_id: Option<PeerID>, // peer's self-selected ID, as described above for the tracker request (string)
    ip: String, // peer's IP address either IPv6 (hexed) or IPv4 (dotted quad) or DNS name (string)
    port: i64, // peer's port number (integer)
}
