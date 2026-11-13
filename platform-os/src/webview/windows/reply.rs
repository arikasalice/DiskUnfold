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

    #[cfg(test)]
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

    /// Posts a reply only as successful delivery when its owning generation remains active.
    ///
    /// The native post call can reenter the STA and revoke the generation before it
    /// returns. In that case the response may have reached the old WebView, but this
    /// reply is conservatively reported as `NotPosted` so the retired generation does
    /// not acknowledge delivery to its replacement.
    pub(super) fn deliver_with_owner<E>(
        mut self,
        post: impl FnOnce(&str) -> Result<(), E>,
        owner_is_active: impl FnOnce() -> bool,
    ) -> Result<(), E> {
        post(&self.message)?;
        if owner_is_active()
            && let Some(callback) = self.callback.take()
        {
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
