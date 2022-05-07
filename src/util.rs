


use std::ffi::CStr;


pub fn safer_cstr(chars: &[std::os::raw::c_char]) -> Option<&CStr> {
    if chars.contains(&0) && chars[0] != 0 {
        Some(unsafe { CStr::from_ptr(&chars[0]) })
    } else {
        None
    }
}