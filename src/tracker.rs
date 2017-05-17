use bip_bencode::{BDecodeOpt, BDictAccess, BRefAccess, BencodeRef};
use byteorder::{BigEndian, ByteOrder};
use errors::*;
use hyper;
use hyper::Url;
use metainfo::*;
use peer_protocol::PeerID;
use std::io::Read;
use std::net;
use std::time::Duration;
use util::QueryParameters;

// Client to talk to a tracker
#[derive(Debug)]
pub struct TrackerClient {
    metainfo: MetaInfo,
    peer_id: PeerID,
    url: Url,
    client: hyper::client::Client,
    tracker_id: Option<String>,
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
               tracker_id: None,
               user_agent: "Bittles/0.01 rust-lang".to_string(),
           })
    }

    pub fn easy_start(&mut self) -> Result<TrackerResponse> {
        let req = TrackerRequest {
            info_hash: self.metainfo.info_hash.clone(),
            peer_id: self.peer_id.clone(),
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
            tracker_id: None,
        };
        self.request(&req)
    }

    fn request(&mut self, req: &TrackerRequest) -> Result<TrackerResponse> {
        let res = {
            let http_req = self.build_req(req)?;
            let mut http_res = http_req.send()?;
            self.parse_res(&mut http_res)?
        };

        // Store the tracker id if given
        self.tracker_id = res.tracker_id.clone().or(self.tracker_id.take());

        Ok(res)
    }

    fn build_req(&self, req: &TrackerRequest) -> Result<hyper::client::RequestBuilder> {
        // TODO set user agent
        let mut qps = QueryParameters::new();
        qps.push("info_hash", req.info_hash.hash);
        qps.push("peer_id", req.peer_id.id);
        qps.push_num("port", req.port);
        qps.push_num("uploaded", req.uploaded);
        qps.push_num("downloaded", req.downloaded);
        qps.push_num("left", req.left);
        qps.push("compact", if req.compact { "1" } else { "0" });
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
        if let Some(ref tracker_id) = req.tracker_id {
            qps.push("trackerid", tracker_id);
        }

        let mut url = self.url.clone();
        qps.apply(&mut url);

        let http_req = self.client
            .get(url)
            .header(hyper::header::UserAgent(self.user_agent.clone()));
        Ok(http_req)
    }

    fn parse_res(&self, http_res: &mut hyper::client::response::Response) -> Result<TrackerResponse> {
        if http_res.status != hyper::status::StatusCode::Ok {
            bail!("tracker returned non-200: {}", http_res.status);
        }
        let mut buf = Vec::new();
        http_res.read_to_end(&mut buf)?;
        let b = BencodeRef::decode(buf.as_slice(), BDecodeOpt::default())?;
        let bd = b.dict().ok_or("response bencoding not a dict")?;
        Ok(TrackerResponse {
               failure_reason: lookup_str(bd, "failure reason".as_bytes())?,
               warning_message: lookup_str(bd, "warning message".as_bytes())?,
               interval: lookup_i64(bd, "interval".as_bytes())?
                   .ok_or("missing 'interval'")?,
               min_interval: lookup_i64(bd, "min interval".as_bytes())?,
               tracker_id: lookup_str(bd, "tracker id".as_bytes())?,
               complete: lookup_i64(bd, "complete".as_bytes())?
                   .ok_or("missing 'complete'")?,
               incomplete: lookup_i64(bd, "incomplete".as_bytes())?
                   .ok_or("missing 'incomplete'")?,
               peers: self.parse_peers(bd)?, // TODO
           })
    }

    /// Parse the peers list from a complete tracker response bdict.
    fn parse_peers(&self, bd: &BDictAccess<BencodeRef>) -> Result<Vec<Peer>> {
        let peers = bd.lookup("peers".as_bytes()).ok_or("missing 'peers'")?;
        if let Some(_) = peers.dict() {
            bail!("Reading peers as 'dict' not implemented");
        }
        if let Some(_) = peers.str() {
            bail!("Reading peers as 'str' not implemented");
        }
        if let Some(_) = peers.list() {
            bail!("Reading peers as 'list' not implemented");
        }
        if let Some(peers) = peers.bytes() {
            // peers: (binary model)
            // multiples of 6 bytes.
            // First 4 bytes are the IP address and last 2 bytes are the port number.
            // All in network (big endian) notation.
            if peers.len() % 6 != 0 {
                bail!("Peers 'byte' representation not 6*n bytes");
            }
            return Ok(peers
                          .chunks(6)
                          .map(|chunk| {
                let (bytes_ip, bytes_port) = chunk.split_at(4);
                let port = BigEndian::read_u16(bytes_port);
                let ip = net::Ipv4Addr::new(bytes_ip[0], bytes_ip[1], bytes_ip[2], bytes_ip[3]);
                Peer {
                    peer_id: None,
                    address: net::SocketAddr::V4(net::SocketAddrV4::new(ip, port)),
                }
            })
                          .collect());
        }
        bail!("wrong type for 'peers': {:?}", peers);
    }
}

fn lookup_str<'a>(dict: &'a BDictAccess<BencodeRef>, key: &'a [u8]) -> Result<Option<String>> {
    match dict.lookup(key) {
        Some(x) => {
            match x.str() {
                Some(x) => Ok(Some(x.to_owned())),
                None => bail!("'{:?}' exists but is not a utf8 string", key),
            }
        }
        None => Ok(None),
    }
}

fn lookup_i64<'a>(dict: &'a BDictAccess<BencodeRef>, key: &'a [u8]) -> Result<Option<i64>> {
    match dict.lookup(key) {
        Some(x) => {
            match x.int() {
                Some(x) => Ok(Some(x)),
                None => bail!("'{:?}' exists but is not an int", key),
            }
        }
        None => Ok(None),
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum TrackerEvent {
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
    pub info_hash: InfoHash, // Hash of the 'info' section of the torrent file
    pub peer_id: PeerID, // Randomly generated peer id
    pub port: i64, // Port the client is listening. Typically in [6881-6889]
    pub uploaded: i64, // Bytes uploaded (since the client sent the 'started' event to the tracker)
    pub downloaded: i64, // Bytes downloaded (since the client sent the 'started' event to the tracker)
    pub left: i64, // Bytes left until 100% downloaded
    pub compact: bool, // Support a compact response
    pub no_peer_id: bool, // Response can omit peer id field in peers dictionary
    pub event: TrackerEvent,
    pub ip: Option<String>, // Outwardly-reachable IP of the client
    pub numwant: Option<i64>, // Number of peers requested
    pub key: Option<String>, // Identifier for this client with the tracker
    pub tracker_id: Option<String>, // If a previous announce contained a tracker id, it should be set here
}

// The tracker responds with "text/plain" document consisting of a bencoded dictionary
#[derive(Debug)]
pub struct TrackerResponse {
    pub failure_reason: Option<String>, // If present, then no other keys may be present. The value is a human-readable error message as to why the request failed (string).
    pub warning_message: Option<String>, // (new, optional) Similar to failure reason, but the response still gets processed normally. The warning message is shown just like an error.
    pub interval: i64, // Interval in seconds that the client should wait between sending regular requests to the tracker
    pub min_interval: Option<i64>, // (optional) Minimum announce interval. If present clients must not reannounce more frequently than this.
    /// A string that the client should send back on its next announcements.
    /// If absent and a previous announce sent a tracker id, do not discard the old value; keep using it.
    pub tracker_id: Option<String>,
    pub complete: i64, // Number of peers with the entire file (seeders)
    pub incomplete: i64, // Number of non-seeder peers (leechers)
    pub peers: Vec<Peer>,
}

// A peer reported by the tracker.
// Contains a peer_id if not in compact form.
#[derive(Debug, Clone)]
pub struct Peer {
    pub peer_id: Option<PeerID>, // peer's self-selected ID, as described above for the tracker request
    pub address: net::SocketAddr,
}
