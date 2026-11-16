//! 中身が手元に無く、読むとクラウドから取ってくるファイル (OneDrive の「オンラインのみ」など) の判定
//! (→ docs/ui.md「画像のプレビュー」)。

use std::path::Path;

/// `path` の中身が手元に無く、読むとクラウドから取ってくるファイルか。属性を見るだけで中身は読まないので、
/// これを確かめてもダウンロードは始まらない。
#[cfg(windows)]
pub(super) fn is_online_only(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS, FILE_ATTRIBUTE_RECALL_ON_OPEN,
    };

    // OneDrive などの Cloud Files の仕組みは RECALL_ON_DATA_ACCESS を、古い階層型ストレージは OFFLINE を付ける。
    const NOT_LOCAL: u32 = FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0
        | FILE_ATTRIBUTE_RECALL_ON_OPEN.0
        | FILE_ATTRIBUTE_OFFLINE.0;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.file_attributes() & NOT_LOCAL != 0)
}

#[cfg(not(windows))]
pub(super) fn is_online_only(_path: &Path) -> bool {
    false
}

/// OneDrive と同じ Cloud Files の仕組みで、中身が手元に無いファイルを作るテスト用の同期のフォルダー。
/// 中身を読まれると OS が `fetch_data` を呼ぶので、その回数でダウンロードが起きたかを確かめる。
#[cfg(all(test, windows))]
pub(super) mod placeholders {
    use std::ffi::c_void;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, MutexGuard, PoisonError};

    use windows::Win32::Foundation::NTSTATUS;
    use windows::Win32::Storage::CloudFilters::{
        CF_CALLBACK_INFO, CF_CALLBACK_PARAMETERS, CF_CALLBACK_REGISTRATION,
        CF_CALLBACK_TYPE_FETCH_DATA, CF_CALLBACK_TYPE_NONE, CF_CONNECT_FLAG_NONE,
        CF_CONNECTION_KEY, CF_CREATE_FLAG_NONE, CF_FS_METADATA, CF_HARDLINK_POLICY_NONE,
        CF_HYDRATION_POLICY, CF_HYDRATION_POLICY_FULL, CF_HYDRATION_POLICY_MODIFIER_NONE,
        CF_INSYNC_POLICY_NONE, CF_OPERATION_INFO, CF_OPERATION_PARAMETERS,
        CF_OPERATION_PARAMETERS_0, CF_OPERATION_PARAMETERS_0_0,
        CF_OPERATION_TRANSFER_DATA_FLAG_NONE, CF_OPERATION_TYPE_TRANSFER_DATA,
        CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC, CF_PLACEHOLDER_CREATE_INFO,
        CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT, CF_POPULATION_POLICY,
        CF_POPULATION_POLICY_ALWAYS_FULL, CF_POPULATION_POLICY_MODIFIER_NONE,
        CF_REGISTER_FLAG_NONE, CF_SYNC_POLICIES, CF_SYNC_REGISTRATION, CfConnectSyncRoot,
        CfCreatePlaceholders, CfDisconnectSyncRoot, CfExecute, CfRegisterSyncRoot,
        CfUnregisterSyncRoot,
    };
    use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_NORMAL, FILE_BASIC_INFO};
    use windows::core::{GUID, HSTRING, w};

    use crate::test_support::TempDir;

    /// 呼び出しの表は、つないでいる間 OS が参照するので、関数の中に置かない。
    static CALLBACKS: [CF_CALLBACK_REGISTRATION; 2] = [
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_FETCH_DATA,
            Callback: Some(fetch_data),
        },
        CF_CALLBACK_REGISTRATION {
            Type: CF_CALLBACK_TYPE_NONE,
            Callback: None,
        },
    ];

    struct Provider {
        files: Vec<(String, Vec<u8>)>,
        fetches: AtomicUsize,
    }

    /// 同期のフォルダーの登録は、同時に行うと「要素が見つかりません」で失敗することがあるので、1つずつにする。
    static REGISTRATION: Mutex<()> = Mutex::new(());

    pub(in crate::api) struct SyncRoot {
        _registration: MutexGuard<'static, ()>,
        key: CF_CONNECTION_KEY,
        root: HSTRING,
        provider: Box<Provider>,
        // 登録を外した後に消すので、最後に置く (フィールドは並び順に drop される)。
        dir: TempDir,
    }

    impl SyncRoot {
        /// `files` (名前と中身) を、中身が手元に無いファイルとして置いた同期のフォルダーを作る。
        pub(in crate::api) fn new(name: &str, files: &[(&str, &[u8])]) -> Self {
            let registration_lock = REGISTRATION.lock().unwrap_or_else(PoisonError::into_inner);
            // 開発用のドライブ (Dev Drive) には Cloud Files の仕組みが付かないので、`target/` でなく OS の一時フォルダーに作る。
            let dir = TempDir::at(std::env::temp_dir().join(format!(
                "weblav-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            )));
            let root = HSTRING::from(dir.path().as_os_str());
            let identity = b"weblav-test";
            let registration = CF_SYNC_REGISTRATION {
                StructSize: size_of::<CF_SYNC_REGISTRATION>() as u32,
                ProviderName: w!("WebLAV test"),
                ProviderVersion: w!("1"),
                SyncRootIdentity: identity.as_ptr().cast(),
                SyncRootIdentityLength: identity.len() as u32,
                FileIdentity: std::ptr::null(),
                FileIdentityLength: 0,
                ProviderId: GUID::from_u128(0x6f1d_2c8e_4b7a_4e19_9a53_0c2f_8d41_b7e6),
            };
            let policies = CF_SYNC_POLICIES {
                StructSize: size_of::<CF_SYNC_POLICIES>() as u32,
                Hydration: CF_HYDRATION_POLICY {
                    Primary: CF_HYDRATION_POLICY_FULL,
                    Modifier: CF_HYDRATION_POLICY_MODIFIER_NONE,
                },
                Population: CF_POPULATION_POLICY {
                    Primary: CF_POPULATION_POLICY_ALWAYS_FULL,
                    Modifier: CF_POPULATION_POLICY_MODIFIER_NONE,
                },
                InSync: CF_INSYNC_POLICY_NONE,
                HardLink: CF_HARDLINK_POLICY_NONE,
                PlaceholderManagement: CF_PLACEHOLDER_MANAGEMENT_POLICY_DEFAULT,
            };
            unsafe { CfRegisterSyncRoot(&root, &registration, &policies, CF_REGISTER_FLAG_NONE) }
                .expect("同期のフォルダーを登録できなかった");
            let provider = Box::new(Provider {
                files: files
                    .iter()
                    .map(|(name, data)| (name.to_string(), data.to_vec()))
                    .collect(),
                fetches: AtomicUsize::new(0),
            });
            let context: *const Provider = &*provider;
            let key = unsafe {
                CfConnectSyncRoot(
                    &root,
                    CALLBACKS.as_ptr(),
                    Some(context.cast::<c_void>()),
                    CF_CONNECT_FLAG_NONE,
                )
            }
            .expect("同期のフォルダーにつなげなかった");
            let sync_root = Self {
                _registration: registration_lock,
                key,
                root,
                provider,
                dir,
            };
            sync_root.create_placeholders();
            sync_root
        }

        fn create_placeholders(&self) {
            let names: Vec<HSTRING> = self
                .provider
                .files
                .iter()
                .map(|(name, _)| HSTRING::from(name.as_str()))
                .collect();
            let mut infos: Vec<CF_PLACEHOLDER_CREATE_INFO> = self
                .provider
                .files
                .iter()
                .zip(&names)
                .map(|((name, data), wide)| CF_PLACEHOLDER_CREATE_INFO {
                    RelativeFileName: windows::core::PCWSTR(wide.as_ptr()),
                    FsMetadata: CF_FS_METADATA {
                        BasicInfo: FILE_BASIC_INFO {
                            FileAttributes: FILE_ATTRIBUTE_NORMAL.0,
                            ..Default::default()
                        },
                        FileSize: data.len() as i64,
                    },
                    // 読まれたときに、どのファイルかを `fetch_data` が名前で引く。
                    FileIdentity: name.as_ptr().cast(),
                    FileIdentityLength: name.len() as u32,
                    Flags: CF_PLACEHOLDER_CREATE_FLAG_MARK_IN_SYNC,
                    ..Default::default()
                })
                .collect();
            unsafe { CfCreatePlaceholders(&self.root, &mut infos, CF_CREATE_FLAG_NONE, None) }
                .expect("中身の無いファイルを作れなかった");
            for info in &infos {
                info.Result.ok().expect("中身の無いファイルを作れなかった");
            }
        }

        pub(in crate::api) fn path(&self) -> &Path {
            self.dir.path()
        }

        /// 中身を取りに来た回数。
        pub(in crate::api) fn fetches(&self) -> usize {
            self.provider.fetches.load(Ordering::SeqCst)
        }
    }

    impl Drop for SyncRoot {
        fn drop(&mut self) {
            unsafe {
                let _ = CfDisconnectSyncRoot(self.key);
                let _ = CfUnregisterSyncRoot(&self.root);
            }
        }
    }

    /// 中身を求められたら、そのファイルの中身を丸ごと渡す。
    unsafe extern "system" fn fetch_data(
        info: *const CF_CALLBACK_INFO,
        _params: *const CF_CALLBACK_PARAMETERS,
    ) {
        let info = unsafe { &*info };
        let provider = unsafe { &*info.CallbackContext.cast::<Provider>() };
        provider.fetches.fetch_add(1, Ordering::SeqCst);
        let identity = unsafe {
            std::slice::from_raw_parts(
                info.FileIdentity.cast::<u8>(),
                info.FileIdentityLength as usize,
            )
        };
        let Some((_, data)) = provider
            .files
            .iter()
            .find(|(name, _)| name.as_bytes() == identity)
        else {
            return;
        };
        let operation = CF_OPERATION_INFO {
            StructSize: size_of::<CF_OPERATION_INFO>() as u32,
            Type: CF_OPERATION_TYPE_TRANSFER_DATA,
            ConnectionKey: info.ConnectionKey,
            TransferKey: info.TransferKey,
            CorrelationVector: info.CorrelationVector,
            SyncStatus: std::ptr::null(),
            RequestKey: info.RequestKey,
        };
        let mut params = CF_OPERATION_PARAMETERS {
            ParamSize: (std::mem::offset_of!(CF_OPERATION_PARAMETERS, Anonymous)
                + size_of::<CF_OPERATION_PARAMETERS_0_0>()) as u32,
            Anonymous: CF_OPERATION_PARAMETERS_0 {
                TransferData: CF_OPERATION_PARAMETERS_0_0 {
                    Flags: CF_OPERATION_TRANSFER_DATA_FLAG_NONE,
                    CompletionStatus: NTSTATUS(0),
                    Buffer: data.as_ptr().cast(),
                    Offset: 0,
                    Length: data.len() as i64,
                },
            },
        };
        let _ = unsafe { CfExecute(&operation, &mut params) };
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::placeholders::SyncRoot;
    use super::*;

    #[test]
    fn reading_an_online_only_file_downloads_it() {
        let root = SyncRoot::new("cloud-read", &[("a.txt", b"hello")]);
        let path = root.path().join("a.txt");
        assert!(is_online_only(&path));
        assert_eq!(root.fetches(), 0, "置いただけで中身を取りに来た");

        assert_eq!(std::fs::read(&path).expect("読めなかった"), b"hello");
        assert!(root.fetches() > 0, "読んでも中身を取りに来なかった");
        assert!(
            !is_online_only(&path),
            "読んだ後も手元に無いままになっている"
        );
    }

    #[test]
    fn a_local_file_is_not_online_only() {
        let dir = crate::test_support::TempDir::new("cloud-local");
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"hello").expect("書けなかった");
        assert!(!is_online_only(&path));
    }
}
