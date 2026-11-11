use super::super::contract::{ResponseDelivery, ResponseDeliveryCallback};

pub(super) struct Reply {
    pub(super) message: String,
    callback: Option<ResponseDeliveryCallback>,
}

impl Reply {
    pub(super) fn ipc(
        id: String,
        response: &str,
        callback: Option<ResponseDeliveryCallback>,
    ) -> Self {
        Self {
            message: format!("{id}\n{response}"),
            callback,
        }
    }

    pub(super) fn plain(message: String) -> Self {
        Self {
            message,
            callback: None,
        }
    }

    pub(super) fn deliver_with<E>(
        mut self,
        post: impl FnOnce(&str) -> Result<(), E>,
    ) -> Result<(), E> {
        post(&self.message)?;
        if let Some(callback) = self.callback.take() {
            callback(ResponseDelivery::Posted);
        }
        Ok(())
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if let Some(callback) = self.callback.take() {
            callback(ResponseDelivery::NotPosted);
        }
    }
}
