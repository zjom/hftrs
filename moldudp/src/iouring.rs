//! io_uring-backed recv loops for the MoldUDP64 client.
//!
//! Behaviourally identical to the std-socket loops in `client.rs`: same
//! buffer pool, same gap detection, same channel handoff. Only the way bytes
//! are pulled off the socket changes — instead of a blocking `recv()` per
//! datagram, each loop drives a per-thread io_uring instance and submits one
//! `Recv` SQE at a time, waiting for the matching CQE.
//!
//! Single-shot recv (rather than `recv_multishot` with a registered buffer
//! ring) is deliberately kept here as the simplest correct first cut. It
//! still removes the libc syscall path and lays the groundwork for the
//! batching variants. Sends remain on plain `socket.send_to` because each
//! re-request worker has a per-server destination, which would need
//! `SendMsg` + `msghdr` plumbing to express through io_uring.
//!
//! Compiled in only when `--features iouring` is set on a Linux target.

use std::io;
use std::net::UdpSocket;
use std::os::fd::AsRawFd;
use std::sync::Arc;

use crossbeam::channel::Sender;
use io_uring::{IoUring, opcode, types};
use tracing::{debug, error, trace};

use crate::client::{
    BUF_SIZE, Datagram, LoopAction, Pool, RetransmissionRequest, process_multicast_datagram,
    process_rereq_datagram,
};

/// Submission/completion queue depth. We only ever have one in-flight recv
/// per loop, so 8 is more than enough headroom.
const RING_DEPTH: u32 = 8;

pub(crate) fn multicast_recv_loop(
    socket: UdpSocket,
    pool: Pool,
    data_tx: Sender<Datagram>,
    req_tx: Sender<RetransmissionRequest>,
    expected_session_ident: &mut Option<[u8; 10]>,
    expected_seq_num: &mut Option<u64>,
) {
    debug!("multicast recv loop started (io_uring)");
    let mut ring = match IoUring::new(RING_DEPTH) {
        Ok(r) => r,
        Err(error) => {
            error!(error = %error, "io_uring init failed; loop exiting");
            return;
        }
    };
    let fd = types::Fd(socket.as_raw_fd());

    loop {
        let mut buf = pool.pop().unwrap_or_else(|| {
            trace!("buffer pool exhausted; falling back to heap allocation");
            vec![0u8; BUF_SIZE].into_boxed_slice()
        });

        let n = match recv_one(&mut ring, fd, &mut buf) {
            Ok(n) => n,
            Err(error) => {
                error!(error = %error, "io_uring multicast recv failed; loop exiting");
                let _ = pool.push(buf);
                break;
            }
        };

        match process_multicast_datagram(
            buf,
            n,
            &pool,
            &data_tx,
            &req_tx,
            expected_session_ident,
            expected_seq_num,
        ) {
            LoopAction::Continue => {}
            LoopAction::Stop => break,
        }
    }
    debug!("multicast recv loop exited (io_uring)");
}

pub(crate) fn rerequest_recv_loop(socket: Arc<UdpSocket>, pool: Pool, data_tx: Sender<Datagram>) {
    debug!("re-request recv loop started (io_uring)");
    let mut ring = match IoUring::new(RING_DEPTH) {
        Ok(r) => r,
        Err(error) => {
            error!(error = %error, "io_uring init failed; loop exiting");
            return;
        }
    };
    let fd = types::Fd(socket.as_raw_fd());

    loop {
        let mut buf = pool.pop().unwrap_or_else(|| {
            trace!("buffer pool exhausted; falling back to heap allocation");
            vec![0u8; BUF_SIZE].into_boxed_slice()
        });

        let n = match recv_one(&mut ring, fd, &mut buf) {
            Ok(n) => n,
            Err(error) => {
                error!(error = %error, "io_uring re-request recv failed; loop exiting");
                let _ = pool.push(buf);
                break;
            }
        };

        match process_rereq_datagram(buf, n, &pool, &data_tx) {
            LoopAction::Continue => {}
            LoopAction::Stop => break,
        }
    }
    debug!("re-request recv loop exited (io_uring)");
}

/// Submit a single `Recv` SQE for `buf`, wait for its completion, and return
/// the number of bytes received. Errors map to `io::Error` exactly the way
/// `UdpSocket::recv` would.
fn recv_one(ring: &mut IoUring, fd: types::Fd, buf: &mut [u8]) -> io::Result<usize> {
    let entry = opcode::Recv::new(fd, buf.as_mut_ptr(), buf.len() as u32).build();

    // SAFETY: `buf` outlives the operation — we don't return until
    // `submit_and_wait` reports the completion below, and we don't relocate
    // the buffer in between.
    unsafe {
        ring.submission()
            .push(&entry)
            .map_err(|e| io::Error::other(format!("io_uring submission push: {e}")))?;
    }
    ring.submit_and_wait(1)?;

    let cqe = ring
        .completion()
        .next()
        .expect("submit_and_wait(1) returned without a completion");
    let result = cqe.result();
    if result < 0 {
        return Err(io::Error::from_raw_os_error(-result));
    }
    Ok(result as usize)
}
