//! 领域 ID newtype（透明字符串语义；序列化与前端 string ID 对齐）

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        $(#[$meta])*
        pub struct $name(String);

        impl $name {
            /// 从已规范化字符串构造
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            /// 内部字符串切片
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// 取出内部字符串
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
    };
}

string_id!(AccountId, "账户 ID");
string_id!(ContactId, "联系人本地 ID");
string_id!(MessageId, "邮件本地 ID（稳定；与远端位置分离）");
string_id!(GroupId, "群档案 ID（群聊 Session 关联投影）");
string_id!(SessionId, "会话 ID（一等公民）");
string_id!(RemoteKey, "协议不透明远端定位键");
string_id!(FolderId, "账号内文件夹本地身份（非导航主轴）");
