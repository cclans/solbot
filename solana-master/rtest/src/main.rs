use {
    min_max_heap::MinMaxHeap,
    std::{
        cmp::Ordering,
        rc::Rc,
        u8,
        io::{self, Read, Write},
        net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
        sync::{Arc},
        time::{Duration,Instant,SystemTime},
    },
    quinn::{
        ClientConfig, Endpoint, EndpointConfig, IdleTimeout, NewConnection, VarInt, WriteError,
    },
    socket2::{Domain, SockAddr, Socket, Type},
    borsh::{BorshDeserialize, BorshSerialize},
    rand::Rng,
};

struct SkipServerVerification;

impl SkipServerVerification {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl rustls::client::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::Certificate,
        _intermediates: &[rustls::Certificate],
        _server_name: &rustls::ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: std::time::SystemTime,
    ) -> Result<rustls::client::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::ServerCertVerified::assertion())
    }
}




#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmutableDeserializedPacket {
    priority: u64,
    sender_stake: u64
}

impl ImmutableDeserializedPacket {
   
    pub fn sender_stake(&self) -> u64 {
        self.sender_stake
    }

    pub fn priority(&self) -> u64 {
        self.priority
    }
}

impl PartialOrd for ImmutableDeserializedPacket {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ImmutableDeserializedPacket {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.priority().cmp(&other.priority()) {
            Ordering::Equal => self.sender_stake().cmp(&other.sender_stake()),
            ordering => ordering,
        }
    }
}

pub struct UnprocessedPacketBatches {
    pub packet_priority_queue: MinMaxHeap<Rc<ImmutableDeserializedPacket>>,
}


impl UnprocessedPacketBatches {

    pub fn with_capacity(capacity: usize) -> Self {
        UnprocessedPacketBatches {
            packet_priority_queue: MinMaxHeap::with_capacity(capacity),
        }
    }
  

    pub fn pop_max(&mut self) -> Option<Rc<ImmutableDeserializedPacket>> {
        self.packet_priority_queue
            .pop_max()
            
    }

    pub fn pop_max_n(&mut self, n: usize) -> Option<Vec<Rc<ImmutableDeserializedPacket>>> {

        let num_to_pop = n;
        Some(
            std::iter::from_fn(|| Some(self.pop_max().unwrap()))
                .take(num_to_pop)
                .collect::<Vec<Rc<ImmutableDeserializedPacket>>>(),
        )
    }


    fn push(&mut self, priority: u64, sender_stake: u64) {
        let packet = Rc::new(ImmutableDeserializedPacket {
            priority: priority,
            sender_stake: sender_stake,
        });
        self.packet_priority_queue.push(packet)
    }
}

pub fn new_with_borsh<T: BorshSerialize>(
    data: T,
) -> Vec<u8> {
    let data = data.try_to_vec().unwrap();
    data
}


fn udp_socket(_reuseaddr: bool) -> io::Result<Socket> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
    Ok(sock)
}

pub fn bind_in_range(ip_addr: IpAddr, port: u16) -> io::Result<(u16, UdpSocket)> {
    let sock_result = udp_socket(false);
    //println!("{:?}", sock_result);

    let sock = sock_result.unwrap();
    let addr = SocketAddr::new(ip_addr, port);
    sock.bind(&SockAddr::from(addr))?;

    let sock: UdpSocket = sock.into();
    let local_port = sock.local_addr()?.port();

    Ok((local_port, sock))
    
}


async fn make_connection(tpu_raw: &str) -> Option<NewConnection> {
    
    //let tpu_raw = "141.95.168.120:8023";

    let tpu_split: Vec<&str> = tpu_raw.split(":").collect();
    let tpu_ip = tpu_split.get(0);
    let tpu_port = tpu_split.get(1).unwrap().parse::<u16>().unwrap();
    let tpu_ip_split: Vec<&str> = tpu_ip.unwrap().split(".").collect();
    
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(tpu_ip_split.get(0).unwrap().parse::<u8>().unwrap(), tpu_ip_split.get(1).unwrap().parse::<u8>().unwrap(), tpu_ip_split.get(2).unwrap().parse::<u8>().unwrap(), tpu_ip_split.get(3).unwrap().parse::<u8>().unwrap())), tpu_port);
    println!("{:?}",addr);

    let mut rng = rand::thread_rng();
    let random_number: u16 = rng.gen_range(1000..=2000);

    let (_, client_socket) = match bind_in_range(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), random_number) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Failed to bind socket: {}", e);
            return None;
        }
    };

    let mut crypto = rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_custom_certificate_verifier(SkipServerVerification::new())
        .with_no_client_auth();
    crypto.enable_early_data = true;

    let mut endpoint =
        create_endpoint(EndpointConfig::default(), client_socket);

        

    let mut config = ClientConfig::new(Arc::new(crypto));
    let transport_config = Arc::get_mut(&mut config.transport).unwrap();
    let timeout = IdleTimeout::from(VarInt::from_u32(2000));
    transport_config.max_idle_timeout(Some(timeout));
    transport_config.keep_alive_interval(Some(Duration::from_millis(1000)));

    endpoint.set_default_client_config(config);

    let connecting = endpoint.connect(addr, "connect").unwrap();
    let connecting_result = connecting.await;
    if connecting_result.is_err() {
        println!("{:?}",connecting_result);
        return None;
    }

    let connection = connecting_result;

    
    return Some(connection.unwrap());
    
}

fn create_endpoint(config: EndpointConfig, client_socket: UdpSocket) -> Endpoint {
    quinn::Endpoint::new(config, None, client_socket).unwrap().0
}

use tokio::net::TcpListener;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[tokio::main]
async fn main() {

    /*let mut buffered_packet_batches = UnprocessedPacketBatches::with_capacity(100);

    buffered_packet_batches.push(1,1);
    buffered_packet_batches.push(3,2);
    buffered_packet_batches.push(3,1);
    buffered_packet_batches.push(1,1);
    buffered_packet_batches.push(1,5);
    buffered_packet_batches.push(1,10);
    buffered_packet_batches.push(2,15);
    buffered_packet_batches.push(1,1);


    //println!("{:?}", buffered_packet_batches.packet_priority_queue);
    //println!("{:?}", buffered_packet_batches.pop_max_n(2));
    let mut retryable_packets = MinMaxHeap::with_capacity(buffered_packet_batches.packet_priority_queue.capacity());
        std::mem::swap(
            &mut buffered_packet_batches.packet_priority_queue,
            &mut retryable_packets,
        );
    println!("{:?}", retryable_packets);

    let mut x = retryable_packets
            .into_vec_asc()
            .into_iter();
    
    println!("{:?}", x);
    //println!("{:?}", new_with_borsh(2030 as u128));*/

   // let port : u16 = 1234;
    //let addr = SocketAddr::new(, port);
    //let x = bind_in_range(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),port);
    //println!("{:?}", x);

    let data : &[u8] = &[
        1, 154, 199, 179,  50, 147,  75, 135, 131,   3, 234,  34,
  178, 124,  75,  86, 123,  91, 238, 213, 145,  21,  94,  52,
   38, 190, 205, 150,  96, 175,  33, 135, 162, 130, 236, 191,
  163, 103, 126,  51, 147, 177, 226, 110, 168, 172, 191,  66,
   56,  28, 201, 187,  59,  66, 143, 252,  15,  74, 162, 161,
  147, 129,  32, 164,   2,   1,   0,   1,   2, 252,  83, 201,
   22,  72,  10, 201, 101, 177, 149,  82, 224, 162,  17,  97,
  177, 137, 233,  44,  67, 115,  50, 203,  85,  29,  34,  51,
   27, 122,  51,  16, 221,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,   0,
    0, 157,   0, 142, 206, 251,  27, 162, 168, 151, 197, 217,
  245, 158, 215, 119, 122, 108, 185, 164,  27, 148,  21, 197,
  233,  25, 204, 160, 158, 190, 216, 174, 190,   1,   1,   2,
    0,   0,  12,   2,   0,   0,   0,  64,  66,  15,   0,   0,
    0,   0,   0
    ];
    /*use reqwest::header::CONTENT_TYPE;
    let client = reqwest::Client::new();
    let s = r#"{"jsonrpc":"2.0","id":1, "method":"getSlot"}"#;
   
    //let request_url = format!();
    //println!("{}", request_url);
    let response = client.post("http://localhost:8899")
    .body(s)
    .header(CONTENT_TYPE, "application/json")
    .send()
    .await;

    //let users = response.json().await;
    println!("{:?}", response);*/


    /*
    "141.95.168.120:8023",
    "54.75.88.244:11003",
    "5.9.143.220:8003",
    "69.67.151.138:8023",
*/

    let tpus_raw = [
        "69.67.150.96:11228",
        "45.250.254.97:11228",
        "202.8.8.42:8013",
        "141.98.216.83:8018",
        "202.8.8.42:8015",
        "45.135.95.56:6009",
        "45.63.115.117:11228",
        "141.98.216.83:8009",
        "158.247.198.237:11228",
        "202.8.8.42:8016",
        "141.98.216.83:8017",
        "64.130.50.45:8016",
        "149.50.98.99:8009",
        "141.98.217.204:8009",
        "141.98.216.83:8013",
        "208.85.17.92:8009",
        "65.20.105.27:11228",
        "93.115.25.194:12009",
        "202.8.8.42:8017",
        "64.130.50.45:8009",
        "202.8.8.42:8011",
        "141.98.216.83:8012",
        "84.32.189.30:12009",
        "139.84.236.171:11228",
        "141.98.216.83:8016",
        "103.50.32.195:11228",
        "69.10.34.250:8009",
        "202.8.8.42:8018",
        "186.233.184.24:11228",
        "149.28.186.225:11228",
        "149.50.110.34:11228",
        "64.130.50.45:8011",
        "192.184.3.27:11228",
        "93.115.25.192:12009",
        "160.202.131.101:8009",
        "64.185.225.82:8015",
        "70.34.196.103:11228",
        "141.98.218.34:11228",
        "45.32.90.26:11228",
        "74.118.140.156:11228",
        "84.32.189.37:12009",
        "93.115.25.120:12009",
        "178.237.58.173:11228",
        "103.50.32.98:11228",
        "103.50.32.77:8029",
        "185.107.94.250:11228",
        "103.28.89.174:11228",
        "148.163.69.250:11228",
        "45.32.126.95:11228",
        "103.50.32.104:8009",
        "79.127.224.11:11228",
        "160.202.131.103:8009",
        "121.127.46.132:11228",
        "72.46.84.5:8015",
        "64.140.169.162:8009",
        "104.243.41.215:8009",
        "145.40.114.251:8010",
        "91.189.176.34:11228",
        "74.118.136.3:11228",
        "107.155.92.114:8009",
        "169.150.242.31:8009",
        "45.76.15.94:8010",
        "216.18.211.10:8015",
        "84.32.189.26:12009",
        "5.199.170.111:12009",
        "5.199.170.18:12009",
        "109.61.94.3:11228",
        "107.155.92.10:8009",
        "176.9.19.179:8009",
        "198.244.228.53:8009",
        "205.209.115.174:8009",
        "202.8.9.108:11228",
        "67.209.52.136:11228",
        "149.28.44.18:8009",
        "74.118.139.6:11228",
        "74.118.139.84:11228",
        "69.2.39.164:8010",
        "91.189.180.150:11228",
        "64.58.236.102:11228",
        "93.115.25.181:12009",
        "139.178.89.7:10009",
        "104.207.149.94:8010",
        "141.98.216.44:8010",
        "202.8.9.72:11228",
        "5.35.114.79:8039",
        "38.58.176.230:11228",
        "194.126.173.158:31361",
        "162.248.226.20:8009",
        "103.50.32.62:8029",
        "204.16.242.229:11228",
        "108.171.217.10:8009",
        "202.8.9.37:8009",
        "95.179.128.111:11228",
        "45.63.18.245:8009",
        "208.91.107.230:8013",
        "45.77.3.223:8009",
        "45.32.201.68:11228",
        "67.209.52.23:8009",
        "65.20.105.212:8009",
        "93.115.25.111:12009",
        "84.32.189.40:12009",
        "207.188.6.69:11228",
        "45.250.255.161:8009",
        "208.76.223.129:8009",
        "195.72.145.238:11228",
        "93.115.25.186:12009",
        "64.130.50.51:8010",
        "67.209.54.179:11228",
        "74.118.139.44:11228",
        "93.115.25.123:12009",
        "208.91.107.86:8015",
        "202.8.10.73:8009",
        "93.115.25.113:12009",
        "80.77.161.214:8009",
        "5.199.170.31:12009",
        "45.63.70.94:11228",
        "202.8.9.110:8009",
        "217.170.200.162:11228",
        "84.32.189.29:12009",
        "65.20.100.25:8009",
        "186.233.187.135:11228",
        "74.118.139.254:11228",
        "141.98.217.155:8009",
        "94.231.78.161:11228",
        "74.222.1.134:11228",
        "103.50.32.76:8009",
        "202.8.8.186:11228",
        "67.209.54.154:11228",
        "64.130.50.72:8009",
        "217.147.40.105:11228",
        "74.118.143.73:11228",
        "216.18.211.210:50009",
        "204.16.242.237:8009",
        
        
        ];

    for i in 0..tpus_raw.len() {
        let beforeConn = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("get millis error");
        
        let conn_opt = make_connection(&tpus_raw[i]).await;
        
        let afterconn = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("get millis error");
        //println!("CONN TIME: {} ms", afterconn.as_millis()-beforeConn.as_millis());

        if let None = conn_opt{
            continue;
        }
        let conn = conn_opt.unwrap();
        println!("{:?}",conn);

        let beforeSend = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("get millis error");

        println!("{:?}",_send_buffer_using_conn(data, &conn).await);

        let afterSend = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("get millis error");
        println!("SEND TIME: {} ms", afterSend.as_millis()-beforeSend.as_millis());
    }

    


}

async fn _send_buffer_using_conn(
    data: &[u8],
    connection: &NewConnection,
) -> Result<(), WriteError> {
    
    let mut send_stream = connection.connection.open_uni().await?;

    send_stream.write_all(data).await?;
    let x = send_stream.finish().await?;
    println!("FINISH RESULT: {:?}",x);
    Ok(())
}