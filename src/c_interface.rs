//! C Interfaces for ucore

use alloc::{rc::Rc, boxed::Box};
use core::cell::RefCell;
use core::slice;

/// Global allocator defined in root
pub use self::allocator::UcoreAllocator;

/// Lang items for bare lib
mod lang {
    use core;

    #[lang = "eh_personality"]
    #[no_mangle]
    extern fn eh_personality() {
    }

    #[lang = "panic_fmt"]
    #[no_mangle]
    extern fn panic_fmt(fmt: core::fmt::Arguments, file: &'static str, line: u32) -> ! {
        eprintln!("\n\nPANIC in {} at line {}:", file, line);
        eprintln!("    {}", fmt);
        use super::ucore::__panic;
        unsafe{ __panic() };
        unreachable!()
    }
}

/// Depends on ucore
mod ucore {
    use super::*;
    extern {
        pub fn __alloc_inode(type_: i32) -> *mut INode;
        pub fn inode_init(inode: &mut INode, ops: &INodeOps, fs: &mut Fs);
        pub fn inode_kill(inode: &mut INode);
        pub fn __alloc_fs(type_: i32) -> *mut Fs;
        pub fn __panic();
    }
    pub const SFS_TYPE: i32 = 0; // TODO
}

// Exports for ucore

static SFS_INODE_OPS: INodeOps = INodeOps::from_rust_inode::<sfs::INode>();
//static SFS_FS: *mut Fs = 0 as *mut _;

#[no_mangle]
pub extern fn sfs_do_mount(dev: *mut Device, fs_store: &mut *mut Fs) -> ErrorCode {
    use self::ucore::*;
    let fs = unsafe{__alloc_fs(SFS_TYPE)};
    let device = unsafe{ Box::from_raw(dev) };  // TODO: fix unsafe
    unsafe{&mut (*fs)}.fs = sfs::SimpleFileSystem::open(device).unwrap();
    *fs_store = fs;
    ErrorCode::Ok
}

// Structs defined in ucore

/// ﻿Abstract low-level file.
///
/// Match struct `inode` in ucore `kern/fs/vfs/inode.h`
#[repr(C)]
struct INode {
    inode: Rc<RefCell<vfs::INode>>,
    // ... fields handled extern
}

/// ﻿Abstract filesystem. (Or device accessible as a file.)
///
/// Match struct `fs` in ucore `kern/fs/vfs/vfs.h`
#[repr(C)]
pub struct Fs {
    fs: Rc<vfs::FileSystem>,
    // ... fields handled extern
}

/// Filesystem-namespace-accessible device.
/// d_io is for both reads and writes; the iobuf will indicates the direction.
///
/// Match struct `device` in ucore `kern/fs/devs/dev.h`
#[repr(C)]
pub struct Device {
    blocks: usize,
    blocksize: usize,
    open: extern fn(&mut Device, flags: u32) -> ErrorCode,
    close: extern fn(&mut Device) -> ErrorCode,
    io: extern fn(&mut Device, buf: &mut IoBuf, is_write: bool) -> ErrorCode,
    ioctl: extern fn(&mut Device, op: i32, data: *mut u8) -> ErrorCode,
}

/// ﻿A buffer Rd/Wr status record
///
/// Match struct `iobuf` in ucore `kern/fs/iobuf.h`
#[repr(C)]
struct IoBuf {
    /// the base addr of buffer (used for Rd/Wr)
    base: *mut u8,
    /// current Rd/Wr position in buffer, will have been incremented by the amount transferred
    offset: i32,
    /// the length of buffer  (used for Rd/Wr)
    len: u32,
    /// current resident length need to Rd/Wr, will have been decremented by the amount transferred.
    resident: u32,
}

/// Information about a file
///
/// Match struct `stat` in ucore `libs/stat.h`
#[repr(C)]
struct Stat {
    /// protection mode and file type
    mode: u32,
    /// number of hard links
    nlinks: u32,
    /// number of blocks file is using
    blocks: u32,
    /// file size (bytes)
    size: u32,
}

/// mask for type of file
const S_IFMT: u32 = 070000;
/// ordinary regular file
const S_IFREG: u32 = 010000;
/// directory
const S_IFDIR: u32 = 020000;
/// symbolic link
const S_IFLNK: u32 = 030000;
/// character device
const S_IFCHR: u32 = 040000;
/// block device
const S_IFBLK: u32 = 050000;

/// ﻿Abstract operations on a inode.
///
/// Match struct `inode_ops` in ucore `kern/fs/vfs/inode.h`
// TODO: Append docs from ucore
#[repr(C)]
pub struct INodeOps {
    magic: u64,
    open: extern fn(&mut INode, flags: u32) -> ErrorCode,
    close: extern fn(&mut INode) -> ErrorCode,
    read: extern fn(&mut INode, &mut IoBuf) -> ErrorCode,
    write: extern fn(&mut INode, &mut IoBuf) -> ErrorCode,
    fstat: extern fn(&mut INode, &mut Stat) -> ErrorCode,
    fsync: extern fn(&mut INode) -> ErrorCode,
    namefile: extern fn(&mut INode, &mut IoBuf) -> ErrorCode,
    getdirentry: extern fn(&mut INode, &mut IoBuf) -> ErrorCode,
    reclaim: extern fn(&mut INode) -> ErrorCode,
    gettype: extern fn(&mut INode, type_store: &mut u32) -> ErrorCode,
    tryseek: extern fn(&mut INode, pos: i32) -> ErrorCode,
    truncate: extern fn(&mut INode, len: i32) -> ErrorCode,
    create: extern fn(&mut INode, name: *const u8, excl: bool, inode_store: &mut &mut INode) -> ErrorCode,
    lookup: extern fn(&mut INode, path: &mut u8, inode_store: &mut &mut INode) -> ErrorCode,
    ioctl: extern fn(&mut INode, op: i32, data: &mut u8) -> ErrorCode,
}

#[repr(i32)]
#[derive(Debug, Eq, PartialEq)]
pub enum ErrorCode {
    Ok = 0,
    Unimplemented = -1,
}

// Wrapper functions

use vfs;
use sfs;

impl AsRef<[u8]> for IoBuf {
    fn as_ref(&self) -> &[u8] {
        unsafe{ slice::from_raw_parts(self.base, self.resident as usize) }
    }
}
impl AsMut<[u8]> for IoBuf {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe{ slice::from_raw_parts_mut(self.base, self.resident as usize) }
    }
}
impl IoBuf {
    fn skip(&mut self, len: usize) {
        assert!(len as u32 <= self.resident);
        self.base = unsafe{ self.base.offset(len as isize) };
        self.offset += len as i32;
        self.resident -= len as u32;
    }
}

impl sfs::Device for Device {
    fn read_at(&mut self, offset: usize, buf: &mut [u8]) -> Option<usize> {
        let mut io_buf = IoBuf {
            base: buf.as_mut_ptr(),
            offset: offset as i32,
            len: buf.len() as u32,
            resident: buf.len() as u32,
        };
        let ret = (self.io)(self, &mut io_buf, false);
        assert_eq!(ret, ErrorCode::Ok);
        Some(buf.len() - io_buf.resident as usize)
    }

    fn write_at(&mut self, offset: usize, buf: &[u8]) -> Option<usize> {
        let mut io_buf = IoBuf {
            base: buf.as_ptr() as *mut u8,
            offset: offset as i32,
            len: buf.len() as u32,
            resident: buf.len() as u32,
        };
        let ret = (self.io)(self, &mut io_buf, true);
        assert_eq!(ret, ErrorCode::Ok);
        Some(buf.len() - io_buf.resident as usize)
    }
}

impl INode {
    fn new() -> *mut Self {
        use self::ucore::*;
        let ptr = unsafe{ __alloc_inode(SFS_TYPE) };
        assert!(!ptr.is_null());
//        inode_init(ptr, &SFS_INODE_OPS as *const _, SFS_FS);
        ptr

    }
    fn drop(&mut self) {
        use self::ucore::*;
        unsafe{ inode_kill(self) };
    }
}

impl From<vfs::FileInfo> for Stat {
    fn from(info: vfs::FileInfo) -> Self {
        Stat {
            mode: info.mode,
            nlinks: 0,
            blocks: info.blocks as u32,
            size: info.size as u32,
        }
    }
}

impl INodeOps {
    const fn from_rust_inode<T: vfs::INode>() -> Self {
        extern fn open(inode: &mut INode, flags: u32) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn close(inode: &mut INode) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn read(inode: &mut INode, buf: &mut IoBuf) -> ErrorCode {
            let inode = &inode.inode;
            let len = inode.borrow().read_at(buf.offset as usize, buf.as_mut()).unwrap();
            buf.skip(len);
            ErrorCode::Ok
        }
        extern fn write(inode: &mut INode, buf: &mut IoBuf) -> ErrorCode {
            let inode = &inode.inode;
            let len = inode.borrow().write_at(buf.offset as usize, buf.as_ref()).unwrap();
            buf.skip(len);
            ErrorCode::Ok
        }
        extern fn fstat(inode: &mut INode, stat: &mut Stat) -> ErrorCode {
            let inode = &inode.inode;
            let info = inode.borrow().info().unwrap();
            *stat = Stat::from(info);
            ErrorCode::Ok
        }
        extern fn fsync(inode: &mut INode) -> ErrorCode {
            inode.inode.borrow_mut().sync().unwrap();
            ErrorCode::Ok
        }
        extern fn namefile(inode: &mut INode, buf: &mut IoBuf) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn getdirentry(inode: &mut INode, buf: &mut IoBuf) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn reclaim(inode: &mut INode) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn gettype(inode: &mut INode, type_store: &mut u32) -> ErrorCode {
            let inode = &inode.inode;
            let info = inode.borrow().info().unwrap();
            *type_store = info.type_ as u32;
            ErrorCode::Ok
        }
        extern fn tryseek(inode: &mut INode, pos: i32) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn truncate(inode: &mut INode, len: i32) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn create(inode: &mut INode, name: *const u8, excl: bool, inode_store: &mut &mut INode) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn lookup(inode: &mut INode, path: &mut u8, inode_store: &mut &mut INode) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        extern fn ioctl(inode: &mut INode, op: i32, data: &mut u8) -> ErrorCode {
            ErrorCode::Unimplemented
        }
        INodeOps {
            magic: 0x8c4ba476,
            open, close, read, write, fstat, fsync, namefile, getdirentry,
            reclaim, gettype, tryseek, truncate, create, lookup, ioctl,
        }
    }
}

mod allocator {
    use alloc::heap::{Alloc, AllocErr, Layout};
    use core::ptr::NonNull;

    pub struct UcoreAllocator {
        pub malloc: unsafe extern fn(size: usize) -> *mut u8,
        pub free: unsafe extern fn(*mut u8),
    }

    unsafe impl<'a> Alloc for &'a UcoreAllocator {
        unsafe fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
            const NULL: *mut u8 = 0 as *mut u8;
            match (self.malloc)(layout.size()) {
                NULL => Err(AllocErr::Exhausted { request: layout }),
                ptr => Ok(ptr),
            }
        }
        unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
            (self.free)(ptr);
        }
    }
}