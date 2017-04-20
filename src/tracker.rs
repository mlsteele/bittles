use hyper::Url;
use hyper;
use metainfo::*;
use ring::rand::SystemRandom;
use std::error::Error;
use std::io::Read;
use std::time::Duration;

// Client to talk to a tracker
#[derive(Debug)]
pub struct TrackerClient {
    metainfo: MetaInfo,
    peer_id: PeerID,
    url: Url,
    client: hyper::client::Client,
    trackerid: Option<String>,
}

impl TrackerClient {
    pub fn new(rand: &SystemRandom, metainfo: MetaInfo, peer_id: PeerID) -> Result<TrackerClient,Box<Error>> {
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
        })
    }

    pub fn easy_start(&self) -> Result<TrackerResponse,Box<Error>> {
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

    fn request(&self, req: &TrackerRequest) -> Result<TrackerResponse,Box<Error>> {
        let http_req = self.build_req(req)?;
        let mut http_res = http_req.send()?;
        let res = self.parse_res(&mut http_res)?;
        // TODO store trackerid if present.
        Ok(res)
    }

    fn build_req(&self, req: &TrackerRequest) -> Result<hyper::client::RequestBuilder,Box<Error>> {
        // let mut url = self.url.clone();
        // let qp = url.query_pairs_mut().clear();
        // qp.append_pair("info_hash", req.info_hash);
        // qp.append_pair("peer_id", req.peer_id);
        // qp.append_pair("port", req.port);
        // qp.append_pair("uploaded", req.uploaded);
        // qp.append_pair("downloaded", req.downloaded);
        // qp.append_pair("left", req.left);
        // if req.compact {
        //     qp.append_pair("compact", "1");
        // }
        // if req.no_peer_id {
        //     qp.append_pair("no_peer_id", "1");
        // }
        // match req.event {
        //     Ok(_) => qp.append_pair("event", req.event.str())
        // }
        // qp.append_pair("ip", req.ip);
        // qp.append_pair("numwant", req.numwant);
        // qp.append_pair("key", req.key);
        // qp.append_pair("trackerid", req.trackerid);
        // let http_req = self.client.get(url);
        // http_req
        Err("TODO")?
    }

    fn parse_res(&self, http_res: &mut hyper::client::response::Response) -> Result<TrackerResponse,Box<Error>> {
        let mut buf = String::new();
        http_res.read_to_string(&mut buf)?;
        Ok(TrackerResponse {
            res: buf,
        })
    }
}

pub const PEERID_SIZE: usize = 20;
pub type PeerID = [u8; PEERID_SIZE];

pub fn new_peer_id(rand: &SystemRandom) -> Result<PeerID,Box<Error>> {
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

enum TrackerEvent {
    Started,
    Stopped,
    Completed,
    Periodical,
}

impl TrackerEvent {
    pub fn str(&self) -> Option<String> {
        match self {
            Started => Some("started".to_string()),
            Stopped => Some("stopped".to_string()),
            Completed => Some("completed".to_string()),
            Periodical => None,
        }
    }
}

// Parameters used in the client->tracker GET request
struct TrackerRequest {
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
struct TrackerResponse {
    res: String,
}
