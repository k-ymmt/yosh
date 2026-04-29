#[allow(dead_code)]
pub mod yosh {
    #[allow(dead_code)]
    pub mod plugin {
        #[allow(dead_code, clippy::all)]
        pub mod types {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            #[repr(u8)]
            #[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
            pub enum ErrorCode {
                Denied,
                InvalidArgument,
                IoFailed,
                NotFound,
                Other,
                Timeout,
                PatternNotAllowed,
            }
            impl ErrorCode {
                pub fn name(&self) -> &'static str {
                    match self {
                        ErrorCode::Denied => "denied",
                        ErrorCode::InvalidArgument => "invalid-argument",
                        ErrorCode::IoFailed => "io-failed",
                        ErrorCode::NotFound => "not-found",
                        ErrorCode::Other => "other",
                        ErrorCode::Timeout => "timeout",
                        ErrorCode::PatternNotAllowed => "pattern-not-allowed",
                    }
                }
                pub fn message(&self) -> &'static str {
                    match self {
                        ErrorCode::Denied => "",
                        ErrorCode::InvalidArgument => "",
                        ErrorCode::IoFailed => "",
                        ErrorCode::NotFound => "",
                        ErrorCode::Other => "",
                        ErrorCode::Timeout => "",
                        ErrorCode::PatternNotAllowed => "",
                    }
                }
            }
            impl ::core::fmt::Debug for ErrorCode {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("ErrorCode")
                        .field("code", &(*self as i32))
                        .field("name", &self.name())
                        .field("message", &self.message())
                        .finish()
                }
            }
            impl ::core::fmt::Display for ErrorCode {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    write!(f, "{} (error {})", self.name(), * self as i32)
                }
            }
            impl std::error::Error for ErrorCode {}
            impl ErrorCode {
                #[doc(hidden)]
                pub unsafe fn _lift(val: u8) -> ErrorCode {
                    if !cfg!(debug_assertions) {
                        return ::core::mem::transmute(val);
                    }
                    match val {
                        0 => ErrorCode::Denied,
                        1 => ErrorCode::InvalidArgument,
                        2 => ErrorCode::IoFailed,
                        3 => ErrorCode::NotFound,
                        4 => ErrorCode::Other,
                        5 => ErrorCode::Timeout,
                        6 => ErrorCode::PatternNotAllowed,
                        _ => panic!("invalid enum discriminant"),
                    }
                }
            }
            /// Identifies a standard I/O stream (stdout or stderr).
            /// Named `io-stream` rather than `stream` because `stream` is a reserved
            /// WIT keyword in the component model 0.3 draft.
            #[repr(u8)]
            #[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
            pub enum IoStream {
                Stdout,
                Stderr,
            }
            impl ::core::fmt::Debug for IoStream {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    match self {
                        IoStream::Stdout => f.debug_tuple("IoStream::Stdout").finish(),
                        IoStream::Stderr => f.debug_tuple("IoStream::Stderr").finish(),
                    }
                }
            }
            impl IoStream {
                #[doc(hidden)]
                pub unsafe fn _lift(val: u8) -> IoStream {
                    if !cfg!(debug_assertions) {
                        return ::core::mem::transmute(val);
                    }
                    match val {
                        0 => IoStream::Stdout,
                        1 => IoStream::Stderr,
                        _ => panic!("invalid enum discriminant"),
                    }
                }
            }
            #[repr(u8)]
            #[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
            pub enum HookName {
                PreExec,
                PostExec,
                OnCd,
                PrePrompt,
            }
            impl ::core::fmt::Debug for HookName {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    match self {
                        HookName::PreExec => f.debug_tuple("HookName::PreExec").finish(),
                        HookName::PostExec => {
                            f.debug_tuple("HookName::PostExec").finish()
                        }
                        HookName::OnCd => f.debug_tuple("HookName::OnCd").finish(),
                        HookName::PrePrompt => {
                            f.debug_tuple("HookName::PrePrompt").finish()
                        }
                    }
                }
            }
            impl HookName {
                #[doc(hidden)]
                pub unsafe fn _lift(val: u8) -> HookName {
                    if !cfg!(debug_assertions) {
                        return ::core::mem::transmute(val);
                    }
                    match val {
                        0 => HookName::PreExec,
                        1 => HookName::PostExec,
                        2 => HookName::OnCd,
                        3 => HookName::PrePrompt,
                        _ => panic!("invalid enum discriminant"),
                    }
                }
            }
            /// Static plugin metadata.
            ///
            /// IMPORTANT: `metadata` is the only export that the host calls
            /// without an active `ShellEnv` binding. Implementations MUST NOT
            /// invoke any `yosh:plugin/*` host import (variables, filesystem,
            /// io) from inside `metadata`. Doing so will receive
            /// `error-code::denied` from a synthetic deny-stub regardless of
            /// the granted capabilities.
            #[derive(Clone)]
            pub struct PluginInfo {
                pub name: _rt::String,
                pub version: _rt::String,
                pub commands: _rt::Vec<_rt::String>,
                pub required_capabilities: _rt::Vec<_rt::String>,
                pub implemented_hooks: _rt::Vec<HookName>,
            }
            impl ::core::fmt::Debug for PluginInfo {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("PluginInfo")
                        .field("name", &self.name)
                        .field("version", &self.version)
                        .field("commands", &self.commands)
                        .field("required-capabilities", &self.required_capabilities)
                        .field("implemented-hooks", &self.implemented_hooks)
                        .finish()
                }
            }
        }
        #[allow(dead_code, clippy::all)]
        pub mod variables {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            pub type ErrorCode = super::super::super::yosh::plugin::types::ErrorCode;
            #[allow(unused_unsafe, clippy::all)]
            /// Outer `result` carries denial; inner `option` distinguishes
            /// "variable not set" from "variable set to empty string".
            pub fn get(name: &str) -> Result<Option<_rt::String>, ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 10]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 10]);
                    let vec0 = name;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/variables@0.1.0")]
                    extern "C" {
                        #[link_name = "get"]
                        fn wit_import(_: *mut u8, _: usize, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1);
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = {
                                let l3 = i32::from(*ptr1.add(1).cast::<u8>());
                                match l3 {
                                    0 => None,
                                    1 => {
                                        let e = {
                                            let l4 = *ptr1.add(2).cast::<*mut u8>();
                                            let l5 = *ptr1.add(6).cast::<usize>();
                                            let len6 = l5;
                                            let bytes6 = _rt::Vec::from_raw_parts(
                                                l4.cast(),
                                                len6,
                                                len6,
                                            );
                                            _rt::string_lift(bytes6)
                                        };
                                        Some(e)
                                    }
                                    _ => _rt::invalid_enum_discriminant(),
                                }
                            };
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l7 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l7 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn set(name: &str, value: &str) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = name;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let vec1 = value;
                    let ptr1 = vec1.as_ptr().cast::<u8>();
                    let len1 = vec1.len();
                    let ptr2 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/variables@0.1.0")]
                    extern "C" {
                        #[link_name = "set"]
                        fn wit_import(
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                        );
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                    ) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1.cast_mut(), len1, ptr2);
                    let l3 = i32::from(*ptr2.add(0).cast::<u8>());
                    match l3 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l4 = i32::from(*ptr2.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l4 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            /// Export a variable to the environment (like `export VAR=val` in the shell).
            /// Named `export-env` rather than `export` because `export` is a reserved
            /// WIT keyword.
            pub fn export_env(name: &str, value: &str) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = name;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let vec1 = value;
                    let ptr1 = vec1.as_ptr().cast::<u8>();
                    let len1 = vec1.len();
                    let ptr2 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/variables@0.1.0")]
                    extern "C" {
                        #[link_name = "export-env"]
                        fn wit_import(
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                        );
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                    ) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1.cast_mut(), len1, ptr2);
                    let l3 = i32::from(*ptr2.add(0).cast::<u8>());
                    match l3 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l4 = i32::from(*ptr2.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l4 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
        }
        #[allow(dead_code, clippy::all)]
        pub mod filesystem {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            pub type ErrorCode = super::super::super::yosh::plugin::types::ErrorCode;
            #[allow(unused_unsafe, clippy::all)]
            pub fn cwd() -> Result<_rt::String, ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 9]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 9]);
                    let ptr0 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/filesystem@0.1.0")]
                    extern "C" {
                        #[link_name = "cwd"]
                        fn wit_import(_: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8) {
                        unreachable!()
                    }
                    wit_import(ptr0);
                    let l1 = i32::from(*ptr0.add(0).cast::<u8>());
                    match l1 {
                        0 => {
                            let e = {
                                let l2 = *ptr0.add(1).cast::<*mut u8>();
                                let l3 = *ptr0.add(5).cast::<usize>();
                                let len4 = l3;
                                let bytes4 = _rt::Vec::from_raw_parts(
                                    l2.cast(),
                                    len4,
                                    len4,
                                );
                                _rt::string_lift(bytes4)
                            };
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l5 = i32::from(*ptr0.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l5 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn set_cwd(path: &str) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/filesystem@0.1.0")]
                    extern "C" {
                        #[link_name = "set-cwd"]
                        fn wit_import(_: *mut u8, _: usize, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1);
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l3 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l3 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
        }
        #[allow(dead_code, clippy::all)]
        pub mod files {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            pub type ErrorCode = super::super::super::yosh::plugin::types::ErrorCode;
            #[repr(C)]
            #[derive(Clone, Copy)]
            pub struct FileStat {
                pub is_file: bool,
                pub is_dir: bool,
                pub is_symlink: bool,
                pub size: u64,
                pub mtime_secs: i64,
            }
            impl ::core::fmt::Debug for FileStat {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("FileStat")
                        .field("is-file", &self.is_file)
                        .field("is-dir", &self.is_dir)
                        .field("is-symlink", &self.is_symlink)
                        .field("size", &self.size)
                        .field("mtime-secs", &self.mtime_secs)
                        .finish()
                }
            }
            #[derive(Clone)]
            pub struct DirEntry {
                pub name: _rt::String,
                pub is_file: bool,
                pub is_dir: bool,
                pub is_symlink: bool,
            }
            impl ::core::fmt::Debug for DirEntry {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("DirEntry")
                        .field("name", &self.name)
                        .field("is-file", &self.is_file)
                        .field("is-dir", &self.is_dir)
                        .field("is-symlink", &self.is_symlink)
                        .finish()
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn read_file(path: &str) -> Result<_rt::Vec<u8>, ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 9]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 9]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "read-file"]
                        fn wit_import(_: *mut u8, _: usize, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1);
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = {
                                let l3 = *ptr1.add(1).cast::<*mut u8>();
                                let l4 = *ptr1.add(5).cast::<usize>();
                                let len5 = l4;
                                _rt::Vec::from_raw_parts(l3.cast(), len5, len5)
                            };
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l6 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l6 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn read_dir(path: &str) -> Result<_rt::Vec<DirEntry>, ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 9]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 9]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "read-dir"]
                        fn wit_import(_: *mut u8, _: usize, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1);
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = {
                                let l3 = *ptr1.add(1).cast::<*mut u8>();
                                let l4 = *ptr1.add(5).cast::<usize>();
                                let base11 = l3;
                                let len11 = l4;
                                let mut result11 = _rt::Vec::with_capacity(len11);
                                for i in 0..len11 {
                                    let base = base11.add(i * 11);
                                    let e11 = {
                                        let l5 = *base.add(0).cast::<*mut u8>();
                                        let l6 = *base.add(4).cast::<usize>();
                                        let len7 = l6;
                                        let bytes7 = _rt::Vec::from_raw_parts(
                                            l5.cast(),
                                            len7,
                                            len7,
                                        );
                                        let l8 = i32::from(*base.add(8).cast::<u8>());
                                        let l9 = i32::from(*base.add(9).cast::<u8>());
                                        let l10 = i32::from(*base.add(10).cast::<u8>());
                                        DirEntry {
                                            name: _rt::string_lift(bytes7),
                                            is_file: _rt::bool_lift(l8 as u8),
                                            is_dir: _rt::bool_lift(l9 as u8),
                                            is_symlink: _rt::bool_lift(l10 as u8),
                                        }
                                    };
                                    result11.push(e11);
                                }
                                _rt::cabi_dealloc(base11, len11 * 11, 1);
                                result11
                            };
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l12 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l12 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn metadata(path: &str) -> Result<FileStat, ErrorCode> {
                unsafe {
                    #[repr(align(8))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 32]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 32]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "metadata"]
                        fn wit_import(_: *mut u8, _: usize, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1);
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = {
                                let l3 = i32::from(*ptr1.add(8).cast::<u8>());
                                let l4 = i32::from(*ptr1.add(9).cast::<u8>());
                                let l5 = i32::from(*ptr1.add(10).cast::<u8>());
                                let l6 = *ptr1.add(16).cast::<i64>();
                                let l7 = *ptr1.add(24).cast::<i64>();
                                FileStat {
                                    is_file: _rt::bool_lift(l3 as u8),
                                    is_dir: _rt::bool_lift(l4 as u8),
                                    is_symlink: _rt::bool_lift(l5 as u8),
                                    size: l6 as u64,
                                    mtime_secs: l7,
                                }
                            };
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l8 = i32::from(*ptr1.add(8).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l8 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn write_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let vec1 = data;
                    let ptr1 = vec1.as_ptr().cast::<u8>();
                    let len1 = vec1.len();
                    let ptr2 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "write-file"]
                        fn wit_import(
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                        );
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                    ) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1.cast_mut(), len1, ptr2);
                    let l3 = i32::from(*ptr2.add(0).cast::<u8>());
                    match l3 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l4 = i32::from(*ptr2.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l4 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn append_file(path: &str, data: &[u8]) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let vec1 = data;
                    let ptr1 = vec1.as_ptr().cast::<u8>();
                    let len1 = vec1.len();
                    let ptr2 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "append-file"]
                        fn wit_import(
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                        );
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                    ) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1.cast_mut(), len1, ptr2);
                    let l3 = i32::from(*ptr2.add(0).cast::<u8>());
                    match l3 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l4 = i32::from(*ptr2.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l4 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn create_dir(path: &str, recursive: bool) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "create-dir"]
                        fn wit_import(_: *mut u8, _: usize, _: i32, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: i32, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(
                        ptr0.cast_mut(),
                        len0,
                        match &recursive {
                            true => 1,
                            false => 0,
                        },
                        ptr1,
                    );
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l3 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l3 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn remove_file(path: &str) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "remove-file"]
                        fn wit_import(_: *mut u8, _: usize, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, ptr1);
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l3 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l3 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            pub fn remove_dir(path: &str, recursive: bool) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = path;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/files@0.1.0")]
                    extern "C" {
                        #[link_name = "remove-dir"]
                        fn wit_import(_: *mut u8, _: usize, _: i32, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: *mut u8, _: usize, _: i32, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(
                        ptr0.cast_mut(),
                        len0,
                        match &recursive {
                            true => 1,
                            false => 0,
                        },
                        ptr1,
                    );
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l3 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l3 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
        }
        #[allow(dead_code, clippy::all)]
        pub mod io {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            pub type IoStream = super::super::super::yosh::plugin::types::IoStream;
            pub type ErrorCode = super::super::super::yosh::plugin::types::ErrorCode;
            #[allow(unused_unsafe, clippy::all)]
            pub fn write(target: IoStream, data: &[u8]) -> Result<(), ErrorCode> {
                unsafe {
                    #[repr(align(1))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 2]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 2]);
                    let vec0 = data;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let ptr1 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/io@0.1.0")]
                    extern "C" {
                        #[link_name = "write"]
                        fn wit_import(_: i32, _: *mut u8, _: usize, _: *mut u8);
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(_: i32, _: *mut u8, _: usize, _: *mut u8) {
                        unreachable!()
                    }
                    wit_import(target.clone() as i32, ptr0.cast_mut(), len0, ptr1);
                    let l2 = i32::from(*ptr1.add(0).cast::<u8>());
                    match l2 {
                        0 => {
                            let e = ();
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l3 = i32::from(*ptr1.add(1).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l3 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
        }
        #[allow(dead_code, clippy::all)]
        pub mod commands {
            #[used]
            #[doc(hidden)]
            static __FORCE_SECTION_REF: fn() = super::super::super::__link_custom_section_describing_imports;
            use super::super::super::_rt;
            pub type ErrorCode = super::super::super::yosh::plugin::types::ErrorCode;
            /// Result of a successful (or process-exit) command run. Extended
            /// in the future by adding new functions, never by changing this
            /// record's shape.
            #[derive(Clone)]
            pub struct ExecOutput {
                pub exit_code: i32,
                pub stdout: _rt::Vec<u8>,
                pub stderr: _rt::Vec<u8>,
            }
            impl ::core::fmt::Debug for ExecOutput {
                fn fmt(
                    &self,
                    f: &mut ::core::fmt::Formatter<'_>,
                ) -> ::core::fmt::Result {
                    f.debug_struct("ExecOutput")
                        .field("exit-code", &self.exit_code)
                        .field("stdout", &self.stdout)
                        .field("stderr", &self.stderr)
                        .finish()
                }
            }
            #[allow(unused_unsafe, clippy::all)]
            /// Run an external program with the given argv, capturing
            /// stdout/stderr and returning the exit code.
            ///
            /// Subject to a 1000ms hard timeout enforced by the host.
            /// Subject to the per-plugin `allowed-commands` pattern allowlist.
            /// CWD is the shell's current directory; environment is the
            /// shell's full environment; stdin is `/dev/null`.
            pub fn exec(
                program: &str,
                args: &[_rt::String],
            ) -> Result<ExecOutput, ErrorCode> {
                unsafe {
                    #[repr(align(4))]
                    struct RetArea([::core::mem::MaybeUninit<u8>; 24]);
                    let mut ret_area = RetArea([::core::mem::MaybeUninit::uninit(); 24]);
                    let vec0 = program;
                    let ptr0 = vec0.as_ptr().cast::<u8>();
                    let len0 = vec0.len();
                    let vec2 = args;
                    let len2 = vec2.len();
                    let layout2 = _rt::alloc::Layout::from_size_align_unchecked(
                        vec2.len() * 8,
                        4,
                    );
                    let result2 = if layout2.size() != 0 {
                        let ptr = _rt::alloc::alloc(layout2).cast::<u8>();
                        if ptr.is_null() {
                            _rt::alloc::handle_alloc_error(layout2);
                        }
                        ptr
                    } else {
                        ::core::ptr::null_mut()
                    };
                    for (i, e) in vec2.into_iter().enumerate() {
                        let base = result2.add(i * 8);
                        {
                            let vec1 = e;
                            let ptr1 = vec1.as_ptr().cast::<u8>();
                            let len1 = vec1.len();
                            *base.add(4).cast::<usize>() = len1;
                            *base.add(0).cast::<*mut u8>() = ptr1.cast_mut();
                        }
                    }
                    let ptr3 = ret_area.0.as_mut_ptr().cast::<u8>();
                    #[cfg(target_arch = "wasm32")]
                    #[link(wasm_import_module = "yosh:plugin/commands@0.1.0")]
                    extern "C" {
                        #[link_name = "exec"]
                        fn wit_import(
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                            _: usize,
                            _: *mut u8,
                        );
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    fn wit_import(
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                        _: usize,
                        _: *mut u8,
                    ) {
                        unreachable!()
                    }
                    wit_import(ptr0.cast_mut(), len0, result2, len2, ptr3);
                    let l4 = i32::from(*ptr3.add(0).cast::<u8>());
                    if layout2.size() != 0 {
                        _rt::alloc::dealloc(result2.cast(), layout2);
                    }
                    match l4 {
                        0 => {
                            let e = {
                                let l5 = *ptr3.add(4).cast::<i32>();
                                let l6 = *ptr3.add(8).cast::<*mut u8>();
                                let l7 = *ptr3.add(12).cast::<usize>();
                                let len8 = l7;
                                let l9 = *ptr3.add(16).cast::<*mut u8>();
                                let l10 = *ptr3.add(20).cast::<usize>();
                                let len11 = l10;
                                ExecOutput {
                                    exit_code: l5,
                                    stdout: _rt::Vec::from_raw_parts(l6.cast(), len8, len8),
                                    stderr: _rt::Vec::from_raw_parts(l9.cast(), len11, len11),
                                }
                            };
                            Ok(e)
                        }
                        1 => {
                            let e = {
                                let l12 = i32::from(*ptr3.add(4).cast::<u8>());
                                super::super::super::yosh::plugin::types::ErrorCode::_lift(
                                    l12 as u8,
                                )
                            };
                            Err(e)
                        }
                        _ => _rt::invalid_enum_discriminant(),
                    }
                }
            }
        }
    }
}
#[allow(dead_code)]
pub mod exports {
    #[allow(dead_code)]
    pub mod yosh {
        #[allow(dead_code)]
        pub mod plugin {
            #[allow(dead_code, clippy::all)]
            pub mod plugin {
                #[used]
                #[doc(hidden)]
                static __FORCE_SECTION_REF: fn() = super::super::super::super::__link_custom_section_describing_imports;
                use super::super::super::super::_rt;
                pub type PluginInfo = super::super::super::super::yosh::plugin::types::PluginInfo;
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_metadata_cabi<T: Guest>() -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let result0 = T::metadata();
                    let ptr1 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    let super::super::super::super::yosh::plugin::types::PluginInfo {
                        name: name2,
                        version: version2,
                        commands: commands2,
                        required_capabilities: required_capabilities2,
                        implemented_hooks: implemented_hooks2,
                    } = result0;
                    let vec3 = (name2.into_bytes()).into_boxed_slice();
                    let ptr3 = vec3.as_ptr().cast::<u8>();
                    let len3 = vec3.len();
                    ::core::mem::forget(vec3);
                    *ptr1.add(4).cast::<usize>() = len3;
                    *ptr1.add(0).cast::<*mut u8>() = ptr3.cast_mut();
                    let vec4 = (version2.into_bytes()).into_boxed_slice();
                    let ptr4 = vec4.as_ptr().cast::<u8>();
                    let len4 = vec4.len();
                    ::core::mem::forget(vec4);
                    *ptr1.add(12).cast::<usize>() = len4;
                    *ptr1.add(8).cast::<*mut u8>() = ptr4.cast_mut();
                    let vec6 = commands2;
                    let len6 = vec6.len();
                    let layout6 = _rt::alloc::Layout::from_size_align_unchecked(
                        vec6.len() * 8,
                        4,
                    );
                    let result6 = if layout6.size() != 0 {
                        let ptr = _rt::alloc::alloc(layout6).cast::<u8>();
                        if ptr.is_null() {
                            _rt::alloc::handle_alloc_error(layout6);
                        }
                        ptr
                    } else {
                        ::core::ptr::null_mut()
                    };
                    for (i, e) in vec6.into_iter().enumerate() {
                        let base = result6.add(i * 8);
                        {
                            let vec5 = (e.into_bytes()).into_boxed_slice();
                            let ptr5 = vec5.as_ptr().cast::<u8>();
                            let len5 = vec5.len();
                            ::core::mem::forget(vec5);
                            *base.add(4).cast::<usize>() = len5;
                            *base.add(0).cast::<*mut u8>() = ptr5.cast_mut();
                        }
                    }
                    *ptr1.add(20).cast::<usize>() = len6;
                    *ptr1.add(16).cast::<*mut u8>() = result6;
                    let vec8 = required_capabilities2;
                    let len8 = vec8.len();
                    let layout8 = _rt::alloc::Layout::from_size_align_unchecked(
                        vec8.len() * 8,
                        4,
                    );
                    let result8 = if layout8.size() != 0 {
                        let ptr = _rt::alloc::alloc(layout8).cast::<u8>();
                        if ptr.is_null() {
                            _rt::alloc::handle_alloc_error(layout8);
                        }
                        ptr
                    } else {
                        ::core::ptr::null_mut()
                    };
                    for (i, e) in vec8.into_iter().enumerate() {
                        let base = result8.add(i * 8);
                        {
                            let vec7 = (e.into_bytes()).into_boxed_slice();
                            let ptr7 = vec7.as_ptr().cast::<u8>();
                            let len7 = vec7.len();
                            ::core::mem::forget(vec7);
                            *base.add(4).cast::<usize>() = len7;
                            *base.add(0).cast::<*mut u8>() = ptr7.cast_mut();
                        }
                    }
                    *ptr1.add(28).cast::<usize>() = len8;
                    *ptr1.add(24).cast::<*mut u8>() = result8;
                    let vec9 = implemented_hooks2;
                    let len9 = vec9.len();
                    let layout9 = _rt::alloc::Layout::from_size_align_unchecked(
                        vec9.len() * 1,
                        1,
                    );
                    let result9 = if layout9.size() != 0 {
                        let ptr = _rt::alloc::alloc(layout9).cast::<u8>();
                        if ptr.is_null() {
                            _rt::alloc::handle_alloc_error(layout9);
                        }
                        ptr
                    } else {
                        ::core::ptr::null_mut()
                    };
                    for (i, e) in vec9.into_iter().enumerate() {
                        let base = result9.add(i * 1);
                        {
                            *base.add(0).cast::<u8>() = (e.clone() as i32) as u8;
                        }
                    }
                    *ptr1.add(36).cast::<usize>() = len9;
                    *ptr1.add(32).cast::<*mut u8>() = result9;
                    ptr1
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_metadata<T: Guest>(arg0: *mut u8) {
                    let l0 = *arg0.add(0).cast::<*mut u8>();
                    let l1 = *arg0.add(4).cast::<usize>();
                    _rt::cabi_dealloc(l0, l1, 1);
                    let l2 = *arg0.add(8).cast::<*mut u8>();
                    let l3 = *arg0.add(12).cast::<usize>();
                    _rt::cabi_dealloc(l2, l3, 1);
                    let l4 = *arg0.add(16).cast::<*mut u8>();
                    let l5 = *arg0.add(20).cast::<usize>();
                    let base8 = l4;
                    let len8 = l5;
                    for i in 0..len8 {
                        let base = base8.add(i * 8);
                        {
                            let l6 = *base.add(0).cast::<*mut u8>();
                            let l7 = *base.add(4).cast::<usize>();
                            _rt::cabi_dealloc(l6, l7, 1);
                        }
                    }
                    _rt::cabi_dealloc(base8, len8 * 8, 4);
                    let l9 = *arg0.add(24).cast::<*mut u8>();
                    let l10 = *arg0.add(28).cast::<usize>();
                    let base13 = l9;
                    let len13 = l10;
                    for i in 0..len13 {
                        let base = base13.add(i * 8);
                        {
                            let l11 = *base.add(0).cast::<*mut u8>();
                            let l12 = *base.add(4).cast::<usize>();
                            _rt::cabi_dealloc(l11, l12, 1);
                        }
                    }
                    _rt::cabi_dealloc(base13, len13 * 8, 4);
                    let l14 = *arg0.add(32).cast::<*mut u8>();
                    let l15 = *arg0.add(36).cast::<usize>();
                    let base16 = l14;
                    let len16 = l15;
                    _rt::cabi_dealloc(base16, len16 * 1, 1);
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_on_load_cabi<T: Guest>() -> *mut u8 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let result0 = T::on_load();
                    let ptr1 = _RET_AREA.0.as_mut_ptr().cast::<u8>();
                    match result0 {
                        Ok(_) => {
                            *ptr1.add(0).cast::<u8>() = (0i32) as u8;
                        }
                        Err(e) => {
                            *ptr1.add(0).cast::<u8>() = (1i32) as u8;
                            let vec2 = (e.into_bytes()).into_boxed_slice();
                            let ptr2 = vec2.as_ptr().cast::<u8>();
                            let len2 = vec2.len();
                            ::core::mem::forget(vec2);
                            *ptr1.add(5).cast::<usize>() = len2;
                            *ptr1.add(1).cast::<*mut u8>() = ptr2.cast_mut();
                        }
                    };
                    ptr1
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn __post_return_on_load<T: Guest>(arg0: *mut u8) {
                    let l0 = i32::from(*arg0.add(0).cast::<u8>());
                    match l0 {
                        0 => {}
                        _ => {
                            let l1 = *arg0.add(1).cast::<*mut u8>();
                            let l2 = *arg0.add(5).cast::<usize>();
                            _rt::cabi_dealloc(l1, l2, 1);
                        }
                    }
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_exec_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                    arg2: *mut u8,
                    arg3: usize,
                ) -> i32 {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let bytes0 = _rt::Vec::from_raw_parts(arg0.cast(), len0, len0);
                    let base4 = arg2;
                    let len4 = arg3;
                    let mut result4 = _rt::Vec::with_capacity(len4);
                    for i in 0..len4 {
                        let base = base4.add(i * 8);
                        let e4 = {
                            let l1 = *base.add(0).cast::<*mut u8>();
                            let l2 = *base.add(4).cast::<usize>();
                            let len3 = l2;
                            let bytes3 = _rt::Vec::from_raw_parts(l1.cast(), len3, len3);
                            _rt::string_lift(bytes3)
                        };
                        result4.push(e4);
                    }
                    _rt::cabi_dealloc(base4, len4 * 8, 4);
                    let result5 = T::exec(_rt::string_lift(bytes0), result4);
                    _rt::as_i32(result5)
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_on_unload_cabi<T: Guest>() {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    T::on_unload();
                }
                pub trait Guest {
                    fn metadata() -> PluginInfo;
                    fn on_load() -> Result<(), _rt::String>;
                    fn exec(command: _rt::String, args: _rt::Vec<_rt::String>) -> i32;
                    fn on_unload();
                }
                #[doc(hidden)]
                macro_rules! __export_yosh_plugin_plugin_0_1_0_cabi {
                    ($ty:ident with_types_in $($path_to_types:tt)*) => {
                        const _ : () = { #[export_name =
                        "yosh:plugin/plugin@0.1.0#metadata"] unsafe extern "C" fn
                        export_metadata() -> * mut u8 { $($path_to_types)*::
                        _export_metadata_cabi::<$ty > () } #[export_name =
                        "cabi_post_yosh:plugin/plugin@0.1.0#metadata"] unsafe extern "C"
                        fn _post_return_metadata(arg0 : * mut u8,) { $($path_to_types)*::
                        __post_return_metadata::<$ty > (arg0) } #[export_name =
                        "yosh:plugin/plugin@0.1.0#on-load"] unsafe extern "C" fn
                        export_on_load() -> * mut u8 { $($path_to_types)*::
                        _export_on_load_cabi::<$ty > () } #[export_name =
                        "cabi_post_yosh:plugin/plugin@0.1.0#on-load"] unsafe extern "C"
                        fn _post_return_on_load(arg0 : * mut u8,) { $($path_to_types)*::
                        __post_return_on_load::<$ty > (arg0) } #[export_name =
                        "yosh:plugin/plugin@0.1.0#exec"] unsafe extern "C" fn
                        export_exec(arg0 : * mut u8, arg1 : usize, arg2 : * mut u8, arg3
                        : usize,) -> i32 { $($path_to_types)*:: _export_exec_cabi::<$ty >
                        (arg0, arg1, arg2, arg3) } #[export_name =
                        "yosh:plugin/plugin@0.1.0#on-unload"] unsafe extern "C" fn
                        export_on_unload() { $($path_to_types)*::
                        _export_on_unload_cabi::<$ty > () } };
                    };
                }
                #[doc(hidden)]
                pub(crate) use __export_yosh_plugin_plugin_0_1_0_cabi;
                #[repr(align(1))]
                struct _RetArea([::core::mem::MaybeUninit<u8>; 40]);
                static mut _RET_AREA: _RetArea = _RetArea(
                    [::core::mem::MaybeUninit::uninit(); 40],
                );
            }
            #[allow(dead_code, clippy::all)]
            pub mod hooks {
                #[used]
                #[doc(hidden)]
                static __FORCE_SECTION_REF: fn() = super::super::super::super::__link_custom_section_describing_imports;
                use super::super::super::super::_rt;
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_pre_exec_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                ) {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let bytes0 = _rt::Vec::from_raw_parts(arg0.cast(), len0, len0);
                    T::pre_exec(_rt::string_lift(bytes0));
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_post_exec_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                    arg2: i32,
                ) {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let bytes0 = _rt::Vec::from_raw_parts(arg0.cast(), len0, len0);
                    T::post_exec(_rt::string_lift(bytes0), arg2);
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_on_cd_cabi<T: Guest>(
                    arg0: *mut u8,
                    arg1: usize,
                    arg2: *mut u8,
                    arg3: usize,
                ) {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    let len0 = arg1;
                    let bytes0 = _rt::Vec::from_raw_parts(arg0.cast(), len0, len0);
                    let len1 = arg3;
                    let bytes1 = _rt::Vec::from_raw_parts(arg2.cast(), len1, len1);
                    T::on_cd(_rt::string_lift(bytes0), _rt::string_lift(bytes1));
                }
                #[doc(hidden)]
                #[allow(non_snake_case)]
                pub unsafe fn _export_pre_prompt_cabi<T: Guest>() {
                    #[cfg(target_arch = "wasm32")] _rt::run_ctors_once();
                    T::pre_prompt();
                }
                pub trait Guest {
                    fn pre_exec(command: _rt::String);
                    fn post_exec(command: _rt::String, exit_code: i32);
                    fn on_cd(old_dir: _rt::String, new_dir: _rt::String);
                    fn pre_prompt();
                }
                #[doc(hidden)]
                macro_rules! __export_yosh_plugin_hooks_0_1_0_cabi {
                    ($ty:ident with_types_in $($path_to_types:tt)*) => {
                        const _ : () = { #[export_name =
                        "yosh:plugin/hooks@0.1.0#pre-exec"] unsafe extern "C" fn
                        export_pre_exec(arg0 : * mut u8, arg1 : usize,) {
                        $($path_to_types)*:: _export_pre_exec_cabi::<$ty > (arg0, arg1) }
                        #[export_name = "yosh:plugin/hooks@0.1.0#post-exec"] unsafe
                        extern "C" fn export_post_exec(arg0 : * mut u8, arg1 : usize,
                        arg2 : i32,) { $($path_to_types)*:: _export_post_exec_cabi::<$ty
                        > (arg0, arg1, arg2) } #[export_name =
                        "yosh:plugin/hooks@0.1.0#on-cd"] unsafe extern "C" fn
                        export_on_cd(arg0 : * mut u8, arg1 : usize, arg2 : * mut u8, arg3
                        : usize,) { $($path_to_types)*:: _export_on_cd_cabi::<$ty >
                        (arg0, arg1, arg2, arg3) } #[export_name =
                        "yosh:plugin/hooks@0.1.0#pre-prompt"] unsafe extern "C" fn
                        export_pre_prompt() { $($path_to_types)*::
                        _export_pre_prompt_cabi::<$ty > () } };
                    };
                }
                #[doc(hidden)]
                pub(crate) use __export_yosh_plugin_hooks_0_1_0_cabi;
            }
        }
    }
}
mod _rt {
    pub use alloc_crate::string::String;
    pub use alloc_crate::vec::Vec;
    pub unsafe fn string_lift(bytes: Vec<u8>) -> String {
        if cfg!(debug_assertions) {
            String::from_utf8(bytes).unwrap()
        } else {
            String::from_utf8_unchecked(bytes)
        }
    }
    pub unsafe fn invalid_enum_discriminant<T>() -> T {
        if cfg!(debug_assertions) {
            panic!("invalid enum discriminant")
        } else {
            core::hint::unreachable_unchecked()
        }
    }
    pub unsafe fn bool_lift(val: u8) -> bool {
        if cfg!(debug_assertions) {
            match val {
                0 => false,
                1 => true,
                _ => panic!("invalid bool discriminant"),
            }
        } else {
            val != 0
        }
    }
    pub unsafe fn cabi_dealloc(ptr: *mut u8, size: usize, align: usize) {
        if size == 0 {
            return;
        }
        let layout = alloc::Layout::from_size_align_unchecked(size, align);
        alloc::dealloc(ptr, layout);
    }
    pub use alloc_crate::alloc;
    #[cfg(target_arch = "wasm32")]
    pub fn run_ctors_once() {
        wit_bindgen_rt::run_ctors_once();
    }
    pub fn as_i32<T: AsI32>(t: T) -> i32 {
        t.as_i32()
    }
    pub trait AsI32 {
        fn as_i32(self) -> i32;
    }
    impl<'a, T: Copy + AsI32> AsI32 for &'a T {
        fn as_i32(self) -> i32 {
            (*self).as_i32()
        }
    }
    impl AsI32 for i32 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for u32 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for i16 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for u16 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for i8 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for u8 {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for char {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    impl AsI32 for usize {
        #[inline]
        fn as_i32(self) -> i32 {
            self as i32
        }
    }
    extern crate alloc as alloc_crate;
}
/// Generates `#[no_mangle]` functions to export the specified type as the
/// root implementation of all generated traits.
///
/// For more information see the documentation of `wit_bindgen::generate!`.
///
/// ```rust
/// # macro_rules! export{ ($($t:tt)*) => (); }
/// # trait Guest {}
/// struct MyType;
///
/// impl Guest for MyType {
///     // ...
/// }
///
/// export!(MyType);
/// ```
#[allow(unused_macros)]
#[doc(hidden)]
macro_rules! __export_plugin_world_impl {
    ($ty:ident) => {
        self::export!($ty with_types_in self);
    };
    ($ty:ident with_types_in $($path_to_types_root:tt)*) => {
        $($path_to_types_root)*::
        exports::yosh::plugin::plugin::__export_yosh_plugin_plugin_0_1_0_cabi!($ty
        with_types_in $($path_to_types_root)*:: exports::yosh::plugin::plugin);
        $($path_to_types_root)*::
        exports::yosh::plugin::hooks::__export_yosh_plugin_hooks_0_1_0_cabi!($ty
        with_types_in $($path_to_types_root)*:: exports::yosh::plugin::hooks);
    };
}
#[doc(inline)]
pub(crate) use __export_plugin_world_impl as export;
#[cfg(target_arch = "wasm32")]
#[link_section = "component-type:wit-bindgen:0.31.0:yosh:plugin@0.1.0:plugin-world:encoded world"]
#[doc(hidden)]
pub static __WIT_BINDGEN_COMPONENT_TYPE: [u8; 1701] = *b"\
\0asm\x0d\0\x01\0\0\x19\x16wit-component-encoding\x04\0\x07\xa2\x0c\x01A\x02\x01\
A\x13\x01B\x0a\x01m\x07\x06denied\x10invalid-argument\x09io-failed\x09not-found\x05\
other\x07timeout\x13pattern-not-allowed\x04\0\x0aerror-code\x03\0\0\x01m\x02\x06\
stdout\x06stderr\x04\0\x09io-stream\x03\0\x02\x01m\x04\x08pre-exec\x09post-exec\x05\
on-cd\x0apre-prompt\x04\0\x09hook-name\x03\0\x04\x01ps\x01p\x05\x01r\x05\x04name\
s\x07versions\x08commands\x06\x15required-capabilities\x06\x11implemented-hooks\x07\
\x04\0\x0bplugin-info\x03\0\x08\x03\x01\x17yosh:plugin/types@0.1.0\x05\0\x02\x03\
\0\0\x0aerror-code\x01B\x0a\x02\x03\x02\x01\x01\x04\0\x0aerror-code\x03\0\0\x01k\
s\x01j\x01\x02\x01\x01\x01@\x01\x04names\0\x03\x04\0\x03get\x01\x04\x01j\0\x01\x01\
\x01@\x02\x04names\x05values\0\x05\x04\0\x03set\x01\x06\x04\0\x0aexport-env\x01\x06\
\x03\x01\x1byosh:plugin/variables@0.1.0\x05\x02\x01B\x08\x02\x03\x02\x01\x01\x04\
\0\x0aerror-code\x03\0\0\x01j\x01s\x01\x01\x01@\0\0\x02\x04\0\x03cwd\x01\x03\x01\
j\0\x01\x01\x01@\x01\x04paths\0\x04\x04\0\x07set-cwd\x01\x05\x03\x01\x1cyosh:plu\
gin/filesystem@0.1.0\x05\x03\x01B\x1a\x02\x03\x02\x01\x01\x04\0\x0aerror-code\x03\
\0\0\x01r\x05\x07is-file\x7f\x06is-dir\x7f\x0ais-symlink\x7f\x04sizew\x0amtime-s\
ecsx\x04\0\x09file-stat\x03\0\x02\x01r\x04\x04names\x07is-file\x7f\x06is-dir\x7f\
\x0ais-symlink\x7f\x04\0\x09dir-entry\x03\0\x04\x01p}\x01j\x01\x06\x01\x01\x01@\x01\
\x04paths\0\x07\x04\0\x09read-file\x01\x08\x01p\x05\x01j\x01\x09\x01\x01\x01@\x01\
\x04paths\0\x0a\x04\0\x08read-dir\x01\x0b\x01j\x01\x03\x01\x01\x01@\x01\x04paths\
\0\x0c\x04\0\x08metadata\x01\x0d\x01j\0\x01\x01\x01@\x02\x04paths\x04data\x06\0\x0e\
\x04\0\x0awrite-file\x01\x0f\x04\0\x0bappend-file\x01\x0f\x01@\x02\x04paths\x09r\
ecursive\x7f\0\x0e\x04\0\x0acreate-dir\x01\x10\x01@\x01\x04paths\0\x0e\x04\0\x0b\
remove-file\x01\x11\x04\0\x0aremove-dir\x01\x10\x03\x01\x17yosh:plugin/files@0.1\
.0\x05\x04\x02\x03\0\0\x09io-stream\x01B\x08\x02\x03\x02\x01\x05\x04\0\x09io-str\
eam\x03\0\0\x02\x03\x02\x01\x01\x04\0\x0aerror-code\x03\0\x02\x01p}\x01j\0\x01\x03\
\x01@\x02\x06target\x01\x04data\x04\0\x05\x04\0\x05write\x01\x06\x03\x01\x14yosh\
:plugin/io@0.1.0\x05\x06\x01B\x09\x02\x03\x02\x01\x01\x04\0\x0aerror-code\x03\0\0\
\x01p}\x01r\x03\x09exit-codez\x06stdout\x02\x06stderr\x02\x04\0\x0bexec-output\x03\
\0\x03\x01ps\x01j\x01\x04\x01\x01\x01@\x02\x07programs\x04args\x05\0\x06\x04\0\x04\
exec\x01\x07\x03\x01\x1ayosh:plugin/commands@0.1.0\x05\x07\x02\x03\0\0\x0bplugin\
-info\x01B\x0c\x02\x03\x02\x01\x08\x04\0\x0bplugin-info\x03\0\0\x01@\0\0\x01\x04\
\0\x08metadata\x01\x02\x01j\0\x01s\x01@\0\0\x03\x04\0\x07on-load\x01\x04\x01ps\x01\
@\x02\x07commands\x04args\x05\0z\x04\0\x04exec\x01\x06\x01@\0\x01\0\x04\0\x09on-\
unload\x01\x07\x04\x01\x18yosh:plugin/plugin@0.1.0\x05\x09\x01B\x08\x01@\x01\x07\
commands\x01\0\x04\0\x08pre-exec\x01\0\x01@\x02\x07commands\x09exit-codez\x01\0\x04\
\0\x09post-exec\x01\x01\x01@\x02\x07old-dirs\x07new-dirs\x01\0\x04\0\x05on-cd\x01\
\x02\x01@\0\x01\0\x04\0\x0apre-prompt\x01\x03\x04\x01\x17yosh:plugin/hooks@0.1.0\
\x05\x0a\x04\x01\x1eyosh:plugin/plugin-world@0.1.0\x04\0\x0b\x12\x01\0\x0cplugin\
-world\x03\0\0\0G\x09producers\x01\x0cprocessed-by\x02\x0dwit-component\x070.216\
.0\x10wit-bindgen-rust\x060.31.0";
#[inline(never)]
#[doc(hidden)]
pub fn __link_custom_section_describing_imports() {
    wit_bindgen_rt::maybe_link_cabi_realloc();
}
