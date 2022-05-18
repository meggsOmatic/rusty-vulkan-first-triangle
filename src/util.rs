


use std::ffi::CStr;


pub fn safer_cstr(chars: &[std::os::raw::c_char]) -> Option<&CStr> {
    if chars.contains(&0) && chars[0] != 0 {
        Some(unsafe { CStr::from_ptr(&chars[0]) })
    } else {
        None
    }
}

pub fn as_byte_slice<T>(thing: &T) -> &[u8] {
    unsafe {
        std::slice::from_raw_parts(thing as *const T as *const u8, std::mem::size_of::<T>())
    }
}