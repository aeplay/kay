use super::id::{broadcast_machine_id, MachineID, RawID};
use super::inbox::Inbox;
use super::messaging::{Message, Packet};
use super::type_registry::ShortTypeId;
use byteorder::{ByteOrder, LittleEndian, WriteBytesExt};
use compact::Compact;
#[cfg(feature = "server")]
use std::net::{TcpListener, TcpStream};
#[cfg(feature = "server")]
use std::thread;
use std::time::Duration;
#[cfg(feature = "browser")]
use stdweb::traits::{IEventTarget, IMessageEvent};
#[cfg(feature = "browser")]
use stdweb::web::{SocketBinaryType, SocketReadyState, TypedArray, WebSocket};
#[cfg(feature = "server")]
use tungstenite::util::NonBlockingError;
#[cfg(feature = "server")]
use tungstenite::{
    accept as websocket_accept, client as websocket_client, Message as WebSocketMessage, WebSocket,
};
#[cfg(feature = "server")]
use url::Url;

/// Represents all networking environment and networking state
/// of an `ActorSystem`
pub struct Networking {
    /// The machine index of this machine within the network of peers
    pub machine_id: MachineID,
    /// The current network turn this machine is in. Used to keep track
    /// if this machine lags behind or runs fast compared to its peers
    pub n_turns: usize,
    network: Vec<&'static str>,
    network_connections: Vec<Option<Connection>>,
}

impl Networking {
    /// Create network environment based on this machines id/index
    /// and all peer addresses (including this machine)
    pub fn new(machine_id: u8, network: Vec<&'static str>) -> Networking {
        Networking {
            machine_id: MachineID(machine_id),
            n_turns: 0,
            network_connections: (0..network.len()).into_iter().map(|_| None).collect(),
            network,
        }
    }

    #[cfg(feature = "server")]
    /// Connect to all peers in the network
    pub fn connect(&mut self) {
        let listener = TcpListener::bind(self.network[self.machine_id.0 as usize]).unwrap();

        // first wait for all larger machine_ids to connect
        for (machine_id, _address) in self.network.iter().enumerate() {
            if machine_id > self.machine_id.0 as usize {
                let stream = listener.accept().unwrap().0;
                let mut websocket = websocket_accept(stream).unwrap();
                self.network_connections[machine_id] = Some(Connection::new(websocket))
            }
        }

        thread::sleep(Duration::from_secs(2));

        // then try to connecto to all smaller machine_ids
        for (machine_id, address) in self.network.iter().enumerate() {
            if machine_id < self.machine_id.0 as usize {
                let stream = TcpStream::connect(address).unwrap();
                let websocket =
                    websocket_client(Url::parse(&format!("ws://{}", address)).unwrap(), stream)
                        .unwrap()
                        .0;
                self.network_connections[machine_id] = Some(Connection::new(websocket))
            }
        }

        println!("All mapped");
    }

    #[cfg(feature = "browser")]
    /// Connect to all peers in the network
    pub fn connect(&mut self) {
        for (machine_id, address) in self.network.iter().enumerate() {
            if machine_id != self.machine_id.0 as usize {
                let websocket = WebSocket::new(&format!("ws://{}", address)).unwrap();
                self.network_connections[machine_id] = Some(Connection::new(websocket));
            }
        }
    }

    /// Finish the current networking turn and wait for peers which lag behind
    /// based on their turn number. This is the main backpressure mechanism.
    pub fn finish_turn(&mut self, inboxes: &mut [Option<Inbox>]) {
        let mut should_sleep = None;

        for maybe_connection in &mut self.network_connections {
            if let Some(Connection { n_turns, .. }) = *maybe_connection {
                if n_turns + 120 < self.n_turns {
                    // ~2s difference
                    should_sleep = Some((
                        Duration::from_millis(((self.n_turns - 120 - n_turns) / 10) as u64),
                        n_turns,
                    ));
                }
            }
        }

        if let Some((duration, _other_n_turns)) = should_sleep {
            // println!(
            //     "Sleeping to let other machine catch up ({} vs. {} turns)",
            //     other_n_turns,
            //     self.n_turns
            // );
            // Try to process extra messages if we are ahead
            self.send_and_receive(inboxes);
            self.send_and_receive(inboxes);
            self.send_and_receive(inboxes);
            #[cfg(feature = "server")]
            ::std::thread::sleep(duration);
        };

        self.n_turns += 1;

        for maybe_connection in &mut self.network_connections {
            if let Some(ref mut connection) = *maybe_connection {
                // write turn end, use 0 as "message type" to distinguish from actual packet
                let mut data = Vec::<u8>::with_capacity(
                    ::std::mem::size_of::<ShortTypeId>() + ::std::mem::size_of::<u32>(),
                );
                data.write_u16::<LittleEndian>(0).unwrap();
                data.write_u32::<LittleEndian>(self.n_turns as u32).unwrap();
                connection.enqueue(data);
                connection.n_turns_since_own_turn = 0;
            }
        }
    }

    /// Send queued outbound messages and take incoming queued messages
    /// and forward them to their local target recipient(s)
    pub fn send_and_receive(&mut self, inboxes: &mut [Option<Inbox>]) {
        for maybe_connection in &mut self.network_connections {
            if let Some(ref mut connection) = *maybe_connection {
                connection.try_send_pending();
                connection.try_receive(inboxes);
            }
        }
    }

    /// Enqueue a new (potentially) outbound packet
    pub fn enqueue<M: Message>(&mut self, message_type_id: ShortTypeId, mut packet: Packet<M>) {
        if self.network.len() == 1 {
            return;
        }

        let packet_size = Compact::total_size_bytes(&packet);
        let total_size = ::std::mem::size_of::<ShortTypeId>() + packet_size;
        let machine_id = packet.recipient_id.machine;

        let connections: Vec<&mut Connection> = if machine_id == broadcast_machine_id() {
            self.network_connections
                .iter_mut()
                .filter_map(|maybe_connection| maybe_connection.as_mut())
                .collect()
        } else {
            vec![
                self.network_connections
                    .get_mut(machine_id.0 as usize)
                    .expect("Expected machine index to exist")
                    .as_mut()
                    .expect("Expected connection to exist for machine"),
            ]
        };

        for connection in connections {
            let mut data = Vec::<u8>::with_capacity(total_size);
            data.write_u16::<LittleEndian>(message_type_id.into())
                .unwrap();
            let packet_pos = data.len();
            data.resize(packet_pos + packet_size, 0);
            
            unsafe {
                // store packet compactly in write queue
                Compact::compact_behind(
                    &mut packet,
                    &mut data[packet_pos] as *mut u8 as *mut Packet<M>,
                );
            }

            connection.enqueue(data);
        }

        ::std::mem::forget(packet);
    }

    /// Return a debug message containing the current local view of
    /// network turn progress of all peers in the network
    pub fn debug_all_n_turns(&self) -> String {
        self.network_connections
            .iter()
            .enumerate()
            .map(|(i, connection)| {
                format!(
                    "{}: {}",
                    i,
                    if i == usize::from(self.machine_id.0) {
                        self.n_turns
                    } else {
                        connection.as_ref().unwrap().n_turns
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(",\n")
    }
}

#[cfg(feature = "server")]
pub struct Connection {
    n_turns: usize,
    n_turns_since_own_turn: usize,
    websocket: WebSocket<TcpStream>,
}

#[cfg(feature = "server")]
impl Connection {
    pub fn new(mut websocket: WebSocket<TcpStream>) -> Connection {
        {
            let tcp_socket = websocket.get_mut();
            tcp_socket.set_nonblocking(true).unwrap();
            tcp_socket.set_read_timeout(None).unwrap();
            tcp_socket.set_write_timeout(None).unwrap();
            tcp_socket.set_nodelay(true).unwrap();
        }
        Connection {
            n_turns: 0,
            n_turns_since_own_turn: 0,
            websocket,
        }
    }

    pub fn enqueue(&mut self, message: Vec<u8>) {
        let recipient_id =
            (&message[::std::mem::size_of::<ShortTypeId>()] as *const u8) as *const RawID;
        // println!(
        //     "Enqueueing message recipient: {:?}, data: {:?}",
        //     unsafe{(*recipient_id)}, message
        // );
        self.websocket
            .write_message(WebSocketMessage::binary(message))
            .unwrap();
    }

    pub fn try_send_pending(&mut self) {
        match self.websocket.write_pending() {
            Ok(()) => {}
            Err(e) => if let Some(real_err) = e.into_non_blocking() {
                panic!("{}", real_err)
            },
        }
    }

    pub fn try_receive(&mut self, inboxes: &mut [Option<Inbox>]) {
        loop {
            let blocked = match self.websocket.read_message() {
                Ok(WebSocketMessage::Binary(data)) => dispatch_message(
                    data,
                    inboxes,
                    &mut self.n_turns,
                    &mut self.n_turns_since_own_turn,
                ),
                Ok(other_message) => panic!("Got a non binary message: {:?}", other_message),
                Err(e) => if let Some(real_err) = e.into_non_blocking() {
                    panic!("{}", real_err)
                } else {
                    true
                },
            };

            if blocked {
                break;
            }
        }
    }
}

fn dispatch_message(
    data: Vec<u8>,
    inboxes: &mut [Option<Inbox>],
    n_turns: &mut usize,
    n_turns_since_own_turn: &mut usize,
) -> bool {
    if data[0] == 0 && data[1] == 0 {
        // this is actually a turn start
        *n_turns = LittleEndian::read_u32(&data[::std::mem::size_of::<ShortTypeId>()..]) as usize;
        *n_turns_since_own_turn += 1;

        // pretend that we're blocked so we only ever process all
        // messages of 5 incoming turns within one of our own turns,
        // applying backpressure
        *n_turns_since_own_turn >= 5
    } else {
        let recipient_id =
            (&data[::std::mem::size_of::<ShortTypeId>()] as *const u8) as *const RawID;

        unsafe {
            // #[cfg(feature = "browser")]
            // {
            //     let debugmsg = format!(
            //         "Receiving packet for actor {:?}. Data: {:?}",
            //         (*recipient_id),
            //         data
            //     );
            //     console!(log, debugmsg);
            // }
            if let Some(ref mut inbox) = inboxes[(*recipient_id).type_id.as_usize()] {
                inbox.put_raw(&data);
            } else {
                // #[cfg(feature = "browser")]
                // {
                //     console!(error, "Yeah that didn't work (no inbox)")
                // }
                panic!(
                    "No inbox for {:?} (coming from network)",
                    (*recipient_id).type_id.as_usize()
                )
            }
        }

        false
    }
}

#[cfg(feature = "browser")]
use std::cell::RefCell;
#[cfg(feature = "browser")]
use std::collections::VecDeque;
#[cfg(feature = "browser")]
use std::rc::Rc;

#[cfg(feature = "browser")]
pub struct Connection {
    n_turns: usize,
    n_turns_since_own_turn: usize,
    websocket: WebSocket,
    in_queue: Rc<RefCell<VecDeque<Vec<u8>>>>,
    before_ready_queue: Vec<Vec<u8>>,
}

#[cfg(feature = "browser")]
use stdweb::web::event::SocketMessageEvent;

#[cfg(feature = "browser")]
impl Connection {
    pub fn new(websocket: WebSocket) -> Connection {
        let in_queue = Rc::new(RefCell::new(VecDeque::new()));
        let in_queue_for_listener = in_queue.clone();

        websocket.set_binary_type(SocketBinaryType::ArrayBuffer);
        websocket.add_event_listener(move |event: SocketMessageEvent| {
            in_queue_for_listener.borrow_mut().push_back({
                let typed_array: TypedArray<u8> = event.data().into_array_buffer().unwrap().into();
                typed_array.to_vec()
            })
        });

        Connection {
            n_turns: 0,
            n_turns_since_own_turn: 0,
            websocket,
            in_queue,
            before_ready_queue: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, message: Vec<u8>) {
        if self.websocket.ready_state() == SocketReadyState::Open {
            for earlier_message in self.before_ready_queue.drain(..) {
                self.websocket.send_bytes(&earlier_message).unwrap()
            }
            self.websocket.send_bytes(&message).unwrap();
        } else {
            self.before_ready_queue.push(message);
        }
    }

    pub fn try_send_pending(&mut self) {
        // nothing to do here
    }

    pub fn try_receive(&mut self, inboxes: &mut [Option<Inbox>]) {
        if let Ok(mut in_queue) = self.in_queue.try_borrow_mut() {
            //console!(log, "Before drain!");
            for message in in_queue.drain(..) {
                //console!(log, "Before dispatch!");
                dispatch_message(
                    message,
                    inboxes,
                    &mut self.n_turns,
                    &mut self.n_turns_since_own_turn,
                );
                //console!(log, "After dispatch!")
            }
        } else {
            //console!(log, "Cannot borrow inqueue mutably!")
        }
    }
}
