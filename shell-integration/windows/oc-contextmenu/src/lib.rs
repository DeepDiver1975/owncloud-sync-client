//! oc-contextmenu: IContextMenu3 COM DLL for ownCloud right-click integration.

#![allow(non_snake_case)]

pub mod menu_builder;

#[cfg(windows)]
mod registration;

#[cfg(windows)]
mod win_impl {
    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::Mutex;
    use windows::core::{implement, ComInterface, IUnknown, GUID, HRESULT, PCWSTR, PSTR};
    use windows::Win32::Foundation::{
        CLASS_E_NOAGGREGATION, E_FAIL, E_POINTER, HINSTANCE, LPARAM, LRESULT, S_FALSE, S_OK, WPARAM,
    };
    use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl, IDataObject};
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::System::Registry::HKEY;
    use windows::Win32::UI::Shell::{
        DragQueryFileW, IContextMenu, IContextMenu2, IContextMenu2_Impl, IContextMenu3,
        IContextMenu3_Impl, IContextMenu_Impl, IShellExtInit, IShellExtInit_Impl,
        CMINVOKECOMMANDINFO,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, InsertMenuW, HMENU, MF_BYPOSITION, MF_GRAYED,
        MF_POPUP, MF_SEPARATOR, MF_STRING,
    };

    use super::menu_builder::{parse_menu_items, MenuItemDef};
    use oc_ipc::PipeConnection;

    pub const CLSID_OC_CONTEXT_MENU: GUID = GUID {
        data1: 0xABCD_0010,
        data2: 0x1234,
        data3: 0x5678,
        data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
    };

    static DLL_REF_COUNT: AtomicI32 = AtomicI32::new(0);
    static mut DLL_HINSTANCE: HINSTANCE = HINSTANCE(0);

    #[no_mangle]
    pub extern "system" fn DllMain(
        hinstance: HINSTANCE,
        reason: u32,
        _reserved: *mut std::ffi::c_void,
    ) -> i32 {
        const DLL_PROCESS_ATTACH: u32 = 1;
        if reason == DLL_PROCESS_ATTACH {
            unsafe { DLL_HINSTANCE = hinstance };
        }
        1
    }

    #[no_mangle]
    pub unsafe extern "system" fn DllGetClassObject(
        rclsid: *const GUID,
        riid: *const GUID,
        ppv: *mut *mut std::ffi::c_void,
    ) -> HRESULT {
        if rclsid.is_null() || riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }
        let clsid = unsafe { &*rclsid };
        let iid = unsafe { &*riid };
        if *clsid != CLSID_OC_CONTEXT_MENU {
            return HRESULT(0x8004_0154_u32 as i32);
        }
        let factory: IUnknown = ContextMenuFactory::new().into();
        factory.query(iid, ppv)
    }

    #[no_mangle]
    pub extern "system" fn DllCanUnloadNow() -> HRESULT {
        if DLL_REF_COUNT.load(Ordering::SeqCst) == 0 {
            S_OK
        } else {
            S_FALSE
        }
    }

    #[no_mangle]
    pub extern "system" fn DllRegisterServer() -> HRESULT {
        match super::registration::register() {
            Ok(()) => S_OK,
            Err(_) => HRESULT(0x8007_0005_u32 as i32),
        }
    }

    #[no_mangle]
    pub extern "system" fn DllUnregisterServer() -> HRESULT {
        match super::registration::unregister() {
            Ok(()) => S_OK,
            Err(_) => HRESULT(0x8007_0005_u32 as i32),
        }
    }

    #[implement(IClassFactory)]
    struct ContextMenuFactory;

    impl ContextMenuFactory {
        fn new() -> Self {
            DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
            ContextMenuFactory
        }
    }

    impl Drop for ContextMenuFactory {
        fn drop(&mut self) {
            DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl IClassFactory_Impl for ContextMenuFactory {
        fn CreateInstance(
            &self,
            outer: Option<&IUnknown>,
            iid: *const GUID,
            ppv: *mut *mut std::ffi::c_void,
        ) -> windows::core::Result<()> {
            if outer.is_some() {
                return Err(CLASS_E_NOAGGREGATION.into());
            }
            let handler: IShellExtInit = OcContextMenu::new().into();
            unsafe { handler.query(iid, ppv).ok() }
        }

        fn LockServer(&self, lock: windows::Win32::Foundation::BOOL) -> windows::core::Result<()> {
            if lock.as_bool() {
                DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
            } else {
                DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
            }
            Ok(())
        }
    }

    #[implement(IShellExtInit, IContextMenu, IContextMenu2, IContextMenu3)]
    pub struct OcContextMenu {
        selected_paths: Mutex<Vec<String>>,
        menu_items: Mutex<Vec<MenuItemDef>>,
    }

    impl OcContextMenu {
        pub fn new() -> Self {
            DLL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
            OcContextMenu {
                selected_paths: Mutex::new(Vec::new()),
                menu_items: Mutex::new(Vec::new()),
            }
        }
    }

    impl Drop for OcContextMenu {
        fn drop(&mut self) {
            DLL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl IShellExtInit_Impl for OcContextMenu {
        fn Initialize(
            &self,
            _pidlfolder: *const windows::Win32::UI::Shell::Common::ITEMIDLIST,
            pdataobj: Option<&IDataObject>,
            _hkeyprogid: HKEY,
        ) -> windows::core::Result<()> {
            let data_obj = pdataobj.ok_or(E_FAIL)?;
            let format_etc = windows::Win32::System::Com::FORMATETC {
                cfFormat: CF_HDROP.0,
                ptd: std::ptr::null_mut(),
                dwAspect: windows::Win32::System::Com::DVASPECT_CONTENT.0,
                lindex: -1,
                tymed: windows::Win32::System::Com::TYMED_HGLOBAL.0 as u32,
            };
            let medium = unsafe { data_obj.GetData(&format_etc)? };
            // SAFETY: tymed was requested as TYMED_HGLOBAL, so the hGlobal union
            // arm is the valid one to read.
            let hdrop = unsafe { windows::Win32::UI::Shell::HDROP(medium.u.hGlobal.0 as isize) };
            let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, None) };

            let mut paths: Vec<String> = Vec::with_capacity(count as usize);
            for i in 0..count {
                let len = unsafe { DragQueryFileW(hdrop, i, None) } as usize + 1;
                let mut buf = vec![0u16; len];
                unsafe { DragQueryFileW(hdrop, i, Some(&mut buf)) };
                paths.push(String::from_utf16_lossy(&buf[..len - 1]).to_string());
            }

            unsafe {
                windows::Win32::System::Ole::ReleaseStgMedium(&medium as *const _ as *mut _);
            }

            let menu_items = if let Some(first_path) = paths.first() {
                PipeConnection::connect()
                    .and_then(|mut conn| {
                        conn.send_command(&format!("GET_MENU_ITEMS:{}", first_path))
                    })
                    .map(|r| parse_menu_items(&r))
                    .unwrap_or_default()
            } else {
                Vec::new()
            };

            *self.selected_paths.lock().unwrap() = paths;
            *self.menu_items.lock().unwrap() = menu_items;
            Ok(())
        }
    }

    impl IContextMenu_Impl for OcContextMenu {
        fn QueryContextMenu(
            &self,
            hmenu: HMENU,
            indexmenu: u32,
            idcmdfirst: u32,
            _idcmdlast: u32,
            _uflags: u32,
        ) -> windows::core::Result<()> {
            let items = self.menu_items.lock().unwrap();
            if items.is_empty() {
                return Ok(());
            }
            let submenu = unsafe { CreatePopupMenu()? };
            for (i, item) in items.iter().enumerate() {
                let label_wide: Vec<u16> = item
                    .label
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let flags = if item.enabled {
                    MF_STRING
                } else {
                    MF_STRING | MF_GRAYED
                };
                let ok = unsafe {
                    AppendMenuW(
                        submenu,
                        flags,
                        idcmdfirst as usize + i,
                        PCWSTR(label_wide.as_ptr()),
                    )
                };
                if ok.is_err() {
                    unsafe {
                        let _ = DestroyMenu(submenu);
                    }
                    return Err(E_FAIL.into());
                }
            }
            unsafe {
                let sep_label: Vec<u16> = vec![0u16];
                InsertMenuW(
                    hmenu,
                    indexmenu,
                    MF_BYPOSITION | MF_SEPARATOR,
                    0,
                    PCWSTR(sep_label.as_ptr()),
                )?;
                let submenu_label: Vec<u16> = "ownCloud"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                InsertMenuW(
                    hmenu,
                    indexmenu + 1,
                    MF_BYPOSITION | MF_POPUP,
                    submenu.0 as usize,
                    PCWSTR(submenu_label.as_ptr()),
                )?;
            }
            Ok(())
        }

        fn GetCommandString(
            &self,
            idcmd: usize,
            utype: u32,
            _preserved: *const u32,
            pszname: PSTR,
            cchmax: u32,
        ) -> windows::core::Result<()> {
            const GCS_VERBW: u32 = 0x0000_0004;
            const GCS_HELPTEXTW: u32 = 0x0000_0005;
            let items = self.menu_items.lock().unwrap();
            let item = items.get(idcmd).ok_or(E_FAIL)?;
            if utype == GCS_VERBW || utype == GCS_HELPTEXTW {
                let text = if utype == GCS_VERBW {
                    &item.command
                } else {
                    &item.label
                };
                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                let len = wide.len().min(cchmax as usize);
                unsafe {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), pszname.0 as *mut u16, len);
                }
            }
            Ok(())
        }

        fn InvokeCommand(&self, pici: *const CMINVOKECOMMANDINFO) -> windows::core::Result<()> {
            if pici.is_null() {
                return Err(E_POINTER.into());
            }
            let ici = unsafe { &*pici };
            if ici.lpVerb.0 as usize > 0xFFFF {
                return Err(E_FAIL.into());
            }
            let cmd_id = ici.lpVerb.0 as usize;
            let items = self.menu_items.lock().unwrap();
            let item = items.get(cmd_id).ok_or(E_FAIL)?;
            let command = item.command.clone();
            drop(items);
            let paths = self.selected_paths.lock().unwrap();
            let first_path = paths.first().cloned().unwrap_or_default();
            drop(paths);
            let wire = format!("{}:{}", command, first_path);
            PipeConnection::connect()
                .and_then(|mut conn| conn.send_command(&wire))
                .map_err(|_| E_FAIL)?;
            Ok(())
        }
    }

    impl IContextMenu2_Impl for OcContextMenu {
        fn HandleMenuMsg(
            &self,
            _umsg: u32,
            _wparam: WPARAM,
            _lparam: LPARAM,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }

    impl IContextMenu3_Impl for OcContextMenu {
        fn HandleMenuMsg2(
            &self,
            _umsg: u32,
            _wparam: WPARAM,
            _lparam: LPARAM,
            _plresult: *mut LRESULT,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }
}

#[cfg(windows)]
pub use win_impl::CLSID_OC_CONTEXT_MENU;
