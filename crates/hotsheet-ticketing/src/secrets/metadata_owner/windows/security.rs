//! Handle-bound current-user/ACL policy. No secret values or ACL repair.

use super::{SecretError, refusal};
use std::{fs::File, io, mem, os::windows::io::AsRawHandle, ptr};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_TOKEN, GENERIC_ALL, GENERIC_WRITE, HANDLE,
    LocalFree,
};
use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION, AclSizeInformation,
    AddAccessAllowedAceEx, CONTAINER_INHERIT_ACE, CopySid, CreateWellKnownSid,
    DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetLengthSid,
    GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetTokenInformation, INHERIT_ONLY_ACE,
    InitializeAcl, InitializeSecurityDescriptor, IsValidAcl, IsValidSecurityDescriptor, IsValidSid,
    OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION, PSID, SE_DACL_PROTECTED, SECURITY_ATTRIBUTES,
    SECURITY_DESCRIPTOR, SECURITY_MAX_SID_SIZE, SetSecurityDescriptorControl,
    SetSecurityDescriptorDacl, SetSecurityDescriptorOwner, TOKEN_QUERY, TOKEN_USER, TokenUser,
    WELL_KNOWN_SID_TYPE, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ALL_ACCESS, FILE_APPEND_DATA, FILE_DELETE_CHILD, FILE_WRITE_ATTRIBUTES,
    FILE_WRITE_DATA, FILE_WRITE_EA, WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::SystemServices::{
    ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, SECURITY_DESCRIPTOR_REVISION,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThread, OpenProcessToken, OpenThreadToken,
};

fn okay(success: i32) -> Result<(), SecretError> {
    if success == 0 {
        Err(io::Error::last_os_error().into())
    } else {
        Ok(())
    }
}
struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: this owns the real token handle returned by Open*Token.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct LocalMemory(*mut core::ffi::c_void);
impl Drop for LocalMemory {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: GetSecurityInfo allocated this descriptor with LocalAlloc.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

#[derive(Clone)]
struct Sid {
    words: Vec<u32>,
    length: u32,
}
impl Sid {
    fn raw(&self) -> PSID {
        self.words.as_ptr().cast_mut().cast()
    }
    fn same(&self, other: &Self) -> bool {
        // SAFETY: both contain validated, aligned, owned SID bytes.
        unsafe { EqualSid(self.raw(), other.raw()) != 0 }
    }
    fn copy(raw: PSID) -> Result<Self, SecretError> {
        // SAFETY: callers provide a live OS-owned descriptor/token/validated ACE SID.
        if raw.is_null() || unsafe { IsValidSid(raw) } == 0 {
            return Err(refusal());
        }
        // SAFETY: IsValidSid accepted the still-live SID.
        let length = unsafe { GetLengthSid(raw) };
        if length == 0 || length > SECURITY_MAX_SID_SIZE {
            return Err(refusal());
        }
        let result = Self {
            words: vec![0; (length as usize).div_ceil(mem::size_of::<u32>())],
            length,
        };
        // SAFETY: aligned destination has at least length bytes and source is valid.
        okay(unsafe { CopySid(length, result.raw(), raw) })?;
        Ok(result)
    }
    fn known(kind: WELL_KNOWN_SID_TYPE) -> Result<Self, SecretError> {
        let mut result = Self {
            words: vec![0; (SECURITY_MAX_SID_SIZE as usize).div_ceil(mem::size_of::<u32>())],
            length: SECURITY_MAX_SID_SIZE,
        };
        // SAFETY: output is aligned and the in/out length describes its full capacity.
        okay(unsafe {
            CreateWellKnownSid(kind, ptr::null_mut(), result.raw(), &mut result.length)
        })?;
        if result.length == 0 || result.length > SECURITY_MAX_SID_SIZE {
            return Err(refusal());
        }
        Ok(result)
    }
}

fn effective_sid() -> Result<Sid, SecretError> {
    let mut raw = ptr::null_mut();
    // SAFETY: pseudo thread handle is borrowed; raw is a live HANDLE out-parameter.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut raw) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NO_TOKEN as i32) {
            return Err(error.into());
        }
        // SAFETY: process pseudo handle is borrowed and raw is initialized for output.
        okay(unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) })?;
    }
    if raw.is_null() {
        return Err(refusal());
    }
    let token = Token(raw);
    let mut required = 0;
    // SAFETY: the initial null buffer asks only for the required byte count.
    let first =
        unsafe { GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required) };
    let error = io::Error::last_os_error();
    let maximum =
        mem::size_of::<TOKEN_USER>() + SECURITY_MAX_SID_SIZE as usize + mem::align_of::<usize>();
    if first != 0
        || error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER as i32)
        || (required as usize) < mem::size_of::<TOKEN_USER>()
        || required as usize > maximum
    {
        return Err(refusal());
    }
    let capacity = required;
    let mut storage = vec![0usize; (capacity as usize).div_ceil(mem::size_of::<usize>())];
    // SAFETY: usize storage provides TOKEN_USER alignment and at least capacity bytes.
    okay(unsafe {
        GetTokenInformation(
            token.0,
            TokenUser,
            storage.as_mut_ptr().cast(),
            capacity,
            &mut required,
        )
    })?;
    if required > capacity || (required as usize) < mem::size_of::<TOKEN_USER>() {
        return Err(refusal());
    }
    // SAFETY: the successful query initialized a sufficiently large aligned TOKEN_USER.
    let user = unsafe { ptr::read(storage.as_ptr().cast::<TOKEN_USER>()) };
    let start = storage.as_ptr() as usize;
    let end = start.checked_add(required as usize).ok_or_else(refusal)?;
    let sid = user.User.Sid as usize;
    if sid < start || sid.checked_add(8).is_none_or(|last| last > end) {
        return Err(refusal());
    }
    // SAFETY: the SID's fixed eight-byte header is within the returned buffer.
    let count = unsafe { *(user.User.Sid.cast::<u8>().add(1)) } as usize;
    let size = 8 + count * 4;
    if size > SECURITY_MAX_SID_SIZE as usize || sid.checked_add(size).is_none_or(|last| last > end)
    {
        return Err(refusal());
    }
    Sid::copy(user.User.Sid)
}

#[derive(Clone)]
pub(super) struct Principal {
    user: Sid,
    system: Sid,
    administrators: Sid,
}
impl Principal {
    pub(super) fn current() -> Result<Self, SecretError> {
        Ok(Self {
            user: effective_sid()?,
            system: Sid::known(WinLocalSystemSid)?,
            administrators: Sid::known(WinBuiltinAdministratorsSid)?,
        })
    }
    pub(super) fn check_current(&self) -> Result<(), SecretError> {
        if !self.user.same(&effective_sid()?) {
            return Err(refusal());
        }
        Ok(())
    }
    fn trusted(&self, sid: &Sid) -> bool {
        self.user.same(sid) || self.system.same(sid) || self.administrators.same(sid)
    }
    pub(super) fn creation(&self, directory: bool) -> Result<CreationSecurity<'_>, SecretError> {
        CreationSecurity::new(&self.user, directory)
    }

    pub(super) fn check(&self, file: &File, strict: bool, parent: bool) -> Result<(), SecretError> {
        let mut owner = ptr::null_mut();
        let mut dacl = ptr::null_mut();
        let mut descriptor = ptr::null_mut();
        // SAFETY: file owns a live handle; all output slots are initialized and live.
        let error = unsafe {
            GetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                ptr::null_mut(),
                &mut dacl,
                ptr::null_mut(),
                &mut descriptor,
            )
        };
        if error != 0 {
            return Err(io::Error::from_raw_os_error(error as i32).into());
        }
        let _memory = LocalMemory(descriptor);
        // SAFETY: successful GetSecurityInfo returned this live descriptor.
        if descriptor.is_null() || unsafe { IsValidSecurityDescriptor(descriptor) } == 0 {
            return Err(refusal());
        }
        let owner = Sid::copy(owner)?;
        if !(self.user.same(&owner) || (parent && self.trusted(&owner))) {
            return Err(refusal());
        }
        let mut present = 0;
        let mut defaulted = 0;
        let mut actual_dacl = ptr::null_mut();
        // SAFETY: descriptor is validated and output slots are correctly typed.
        okay(unsafe {
            GetSecurityDescriptorDacl(descriptor, &mut present, &mut actual_dacl, &mut defaulted)
        })?;
        if present == 0 || dacl.is_null() || actual_dacl != dacl {
            return Err(refusal());
        }
        let mut control = 0;
        let mut revision = 0;
        // SAFETY: validated descriptor and initialized scalar outputs.
        okay(unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) })?;
        if strict && control & SE_DACL_PROTECTED == 0 {
            return Err(refusal());
        }
        self.check_acl(dacl, strict)
    }

    fn check_acl(&self, dacl: *const ACL, strict: bool) -> Result<(), SecretError> {
        // SAFETY: dacl belongs to the still-live descriptor returned by the OS.
        if unsafe { IsValidAcl(dacl) } == 0 {
            return Err(refusal());
        }
        let mut info = ACL_SIZE_INFORMATION::default();
        // SAFETY: initialized aligned output matches AclSizeInformation.
        okay(unsafe {
            GetAclInformation(
                dacl,
                (&mut info as *mut ACL_SIZE_INFORMATION).cast(),
                mem::size_of_val(&info) as u32,
                AclSizeInformation,
            )
        })?;
        let begin = dacl as usize;
        let end = begin
            .checked_add(info.AclBytesInUse as usize)
            .ok_or_else(refusal)?;
        let mut owner_access = 0;
        let writes = GENERIC_WRITE
            | GENERIC_ALL
            | FILE_WRITE_DATA
            | FILE_APPEND_DATA
            | FILE_WRITE_EA
            | FILE_WRITE_ATTRIBUTES
            | FILE_DELETE_CHILD
            | DELETE
            | WRITE_DAC
            | WRITE_OWNER;
        for index in 0..info.AceCount {
            let mut ace = ptr::null_mut();
            // SAFETY: index is bounded by the validated ACL's actual ACE count.
            okay(unsafe { GetAce(dacl, index, &mut ace) })?;
            let address = ace as usize;
            if address < begin
                || address
                    .checked_add(mem::size_of::<ACE_HEADER>())
                    .is_none_or(|last| last > end)
            {
                return Err(refusal());
            }
            // SAFETY: complete ACE_HEADER was range checked; unaligned read is allowed.
            let header = unsafe { ptr::read_unaligned(ace.cast::<ACE_HEADER>()) };
            let size = header.AceSize as usize;
            if size < mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart) + 8
                || address.checked_add(size).is_none_or(|last| last > end)
                || ![ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE]
                    .contains(&u32::from(header.AceType))
            {
                return Err(refusal());
            }
            let raw = ace.cast::<u8>();
            let offset = mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart);
            // SAFETY: fixed ACE layout and SID header fit in the checked ACE range.
            let mask =
                unsafe { ptr::read_unaligned(raw.add(mem::size_of::<ACE_HEADER>()).cast::<u32>()) };
            // SAFETY: the SID's fixed header was included in the checked ACE size.
            let count = unsafe { *raw.add(offset + 1) } as usize;
            let sid_length = 8 + count * 4;
            if sid_length > SECURITY_MAX_SID_SIZE as usize || offset + sid_length > size {
                return Err(refusal());
            }
            // SAFETY: complete SID fits in this live, validated ACL entry.
            let sid = Sid::copy(unsafe { raw.add(offset).cast() })?;
            if u32::from(header.AceType) == ACCESS_DENIED_ACE_TYPE {
                if strict {
                    return Err(refusal());
                }
                continue;
            }
            if !strict && u32::from(header.AceFlags) & INHERIT_ONLY_ACE != 0 {
                continue;
            }
            if self.user.same(&sid) {
                owner_access |= mask;
            }
            if (strict && mask != 0 && !self.user.same(&sid))
                || (mask & writes != 0 && !self.trusted(&sid))
            {
                return Err(refusal());
            }
        }
        if strict
            && owner_access & GENERIC_ALL == 0
            && owner_access & FILE_ALL_ACCESS != FILE_ALL_ACCESS
        {
            return Err(refusal());
        }
        Ok(())
    }
}

pub(super) struct CreationSecurity<'a> {
    descriptor: Box<SECURITY_DESCRIPTOR>,
    _acl: Vec<u32>,
    _user: &'a Sid,
}
impl<'a> CreationSecurity<'a> {
    fn new(user: &'a Sid, directory: bool) -> Result<Self, SecretError> {
        let length = mem::size_of::<ACL>()
            + mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart)
            + user.length as usize;
        let mut acl = vec![0u32; length.div_ceil(mem::size_of::<u32>())];
        let mut descriptor = Box::new(SECURITY_DESCRIPTOR::default());
        let pointer = (&mut *descriptor as *mut SECURITY_DESCRIPTOR).cast();
        let flags = if directory {
            OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE
        } else {
            0
        };
        // SAFETY: aligned buffers are large enough; pointers remain owned by this object
        // and the user reference for the lifetime of every CreateFile/Directory call.
        unsafe {
            okay(InitializeAcl(
                acl.as_mut_ptr().cast(),
                length as u32,
                ACL_REVISION,
            ))?;
            okay(AddAccessAllowedAceEx(
                acl.as_mut_ptr().cast(),
                ACL_REVISION,
                flags,
                FILE_ALL_ACCESS,
                user.raw(),
            ))?;
            okay(InitializeSecurityDescriptor(
                pointer,
                SECURITY_DESCRIPTOR_REVISION,
            ))?;
            okay(SetSecurityDescriptorOwner(pointer, user.raw(), 0))?;
            okay(SetSecurityDescriptorDacl(
                pointer,
                1,
                acl.as_ptr().cast(),
                0,
            ))?;
            okay(SetSecurityDescriptorControl(
                pointer,
                SE_DACL_PROTECTED,
                SE_DACL_PROTECTED,
            ))?;
        }
        Ok(Self {
            descriptor,
            _acl: acl,
            _user: user,
        })
    }
    pub(super) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: (&*self.descriptor as *const SECURITY_DESCRIPTOR)
                .cast_mut()
                .cast(),
            bInheritHandle: 0,
        }
    }
}

#[cfg(test)]
mod tests;
