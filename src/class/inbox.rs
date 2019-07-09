use chunky;
use compact::Compact;
use crate::messaging::{Message, Packet};
use crate::type_registry::{ShortTypeId, TypeRegistry};
use crate::tuning::Tuning;
use ::std::rc::Rc;

pub struct Inbox {
    queue: chunky::Queue,
}

impl Inbox {
    pub fn new(ident: &chunky::Ident, storage: Rc<dyn chunky::ChunkStorage>, tuning: &Tuning) -> Self {
        Inbox {
            queue: chunky::Queue::new(ident, tuning.inbox_queue_chunk_size, storage),
        }
    }

    pub fn put<M: Message>(&mut self, mut packet: Packet<M>, message_registry: &TypeRegistry) {
        let packet_size = packet.total_size_bytes();
        let total_size = ::std::mem::size_of::<ShortTypeId>() + packet_size;

        #[allow(clippy::cast_ptr_alignment)]
        unsafe {
            // "Allocate" the space in the queue
            let queue_ptr = self.queue.enqueue(total_size);

            // Write message type
            *(queue_ptr as *mut ShortTypeId) = message_registry.get::<M>();
            let payload_ptr = (queue_ptr as *mut u8).offset(::std::mem::size_of::<ShortTypeId>() as isize);

            // Write the packet into the queue
            Compact::compact_behind(&mut packet, payload_ptr as *mut Packet<M>);
            ::std::mem::forget(packet);
        }
    }

    pub fn len(&self) -> usize {
        self.queue.len()
    }

    pub fn put_raw(&mut self, buf: &[u8]) {
        unsafe {
            let queue_ptr = self.queue.enqueue(buf.len());

            ::std::ptr::copy_nonoverlapping(&buf[0], queue_ptr as *mut u8, buf.len())
        }
    }

    pub fn drain(&mut self) -> InboxIterator {
        InboxIterator {
            n_messages_to_read: self.queue.len(),
            queue: &mut self.queue,
        }
    }
}

pub struct InboxIterator<'a> {
    queue: &'a mut chunky::Queue,
    n_messages_to_read: usize,
}

pub struct DispatchablePacket {
    pub message_type: ShortTypeId,
    pub packet_ptr: *const (),
}

impl<'a> Iterator for InboxIterator<'a> {
    type Item = DispatchablePacket;

    fn next(&mut self) -> Option<DispatchablePacket> {
        if self.n_messages_to_read == 0 {
            None
        } else {
            #[allow(clippy::cast_ptr_alignment)]
            unsafe {
                let ptr = self
                    .queue
                    .dequeue()
                    .expect("should have something left for sure");
                let message_type = *(ptr as *mut ShortTypeId);
                let payload_ptr = (ptr as *mut u8).offset(::std::mem::size_of::<ShortTypeId>() as isize);
                self.n_messages_to_read -= 1;
                Some(DispatchablePacket {
                    message_type,
                    packet_ptr: payload_ptr as *const (),
                })
            }
        }
    }
}

impl<'a> Drop for InboxIterator<'a> {
    fn drop(&mut self) {
        unsafe { self.queue.drop_old_chunks() };
    }
}
