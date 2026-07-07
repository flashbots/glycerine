use std::{collections::HashSet, ffi::CString, io, ptr, slice};

use libc::nlmsghdr;
use nftnl::{
    FinalizedBatch,
    Rule,
    nftnl_sys::{self, libc},
};

// NfQueue -------------------------------------------------------------

pub(super) struct NfQueue {
    pub(super) num: u16,
}

impl nftnl::expr::Expression for NfQueue {
    fn to_expr(&self, _rule: &Rule) -> ptr::NonNull<nftnl_sys::nftnl_expr> {
        let expr = ptr::NonNull::new(unsafe { nftnl_sys::nftnl_expr_alloc(c"queue".as_ptr()) })
            .unwrap_or_else(|| std::process::abort());

        unsafe {
            nftnl_sys::nftnl_expr_set_u16(
                expr.as_ptr(),
                nftnl_sys::NFTNL_EXPR_QUEUE_NUM as u16,
                self.num,
            );
            nftnl_sys::nftnl_expr_set_u16(
                expr.as_ptr(),
                nftnl_sys::NFTNL_EXPR_QUEUE_TOTAL as u16,
                1,
            );
        }

        expr
    }
}

// helpers -------------------------------------------------------------

#[allow(dead_code)]
pub(super) fn get_tables() -> io::Result<HashSet<CString>> {
    let socket = mnl::Socket::new(mnl::Bus::Netfilter)?;
    let portid = socket.portid();
    let seq = 1;

    let batch = nftnl::table::get_tables_nlmsg(seq);
    socket.send(&batch)?;

    let mut buffer = new_buffer();
    let buffer = unsafe {
        slice::from_raw_parts_mut(
            buffer.as_mut_ptr() as *mut u8,
            buffer.len() * size_of::<libc::nlmsghdr>(),
        )
    };

    let mut tables = HashSet::new();
    for message in socket.recv(&mut buffer[..])? {
        let message = message?;
        mnl::cb_run2(message, seq, portid, nftnl::table::get_tables_cb, &mut tables)?;
    }

    Ok(tables)
}

pub(super) fn send_and_process(batch: &FinalizedBatch) -> io::Result<()> {
    let socket = mnl::Socket::new(mnl::Bus::Netfilter)?;
    let portid = socket.portid();

    socket.send_all(batch)?;

    let mut buffer = new_buffer();
    let mut expected_seqs = batch.sequence_numbers();

    while !expected_seqs.is_empty() {
        let buffer = unsafe {
            slice::from_raw_parts_mut(
                buffer.as_mut_ptr() as *mut u8,
                buffer.len() * size_of::<libc::nlmsghdr>(),
            )
        };

        for message in socket.recv(buffer)? {
            let message = message?;
            let expected_seq = expected_seqs.next().expect("unexpected ACK");
            mnl::cb_run(message, expected_seq, portid)?;
        }
    }

    Ok(())
}

fn new_buffer() -> Vec<nlmsghdr> {
    vec![
        libc::nlmsghdr { nlmsg_len: 0, nlmsg_type: 0, nlmsg_flags: 0, nlmsg_seq: 0, nlmsg_pid: 0 };
        (nftnl::nft_nlmsg_maxsize() as usize).div_ceil(size_of::<libc::nlmsghdr>())
    ]
}
