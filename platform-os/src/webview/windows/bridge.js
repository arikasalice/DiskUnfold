(() => {
    if (location.href !== 'https://diskunfold.invalid/index.html' || window !== window.top) return;
    const channel = chrome.webview;
    const pending = new Map();
    let sequence = 0;
    channel.addEventListener('message', ({data}) => {
        if (typeof data !== 'string') return;
        const split = data.indexOf('\n');
        if (split < 0) return;
        const id = data.slice(0, split);
        if (id.startsWith('folder-drop:')) {
            let detail;
            try { detail = JSON.parse(data.slice(split + 1)); } catch { return; }
            folderDrop({ ...detail, seq: Number(id.slice('folder-drop:'.length)) });
            return;
        }
        if (id === 'event') {
            let detail;
            try { detail = JSON.parse(data.slice(split + 1)); } catch { return; }
            window.dispatchEvent(new CustomEvent('du://event', { detail }));
            return;
        }
        const request = pending.get(id);
        if (!request) return;
        pending.delete(id);
        clearTimeout(request.timer);
        const response = data.slice(split + 1);
        if (response) request.resolve(response);
        else request.reject(new Error('DiskUnfold IPC request denied'));
    });
    const folderDrop = (detail) => window.dispatchEvent(new CustomEvent('du://folder-drop', { detail }));
    const inDropZone = (target) => target instanceof Element && !!target.closest('[data-diskunfold-dropzone]');
    document.addEventListener('dragover', (event) => {
        event.preventDefault();
        if (event.dataTransfer) event.dataTransfer.dropEffect = event.isTrusted && inDropZone(event.target) ? 'copy' : 'none';
    }, true);
    document.addEventListener('drop', (event) => {
        event.preventDefault();
        if (!event.isTrusted || !inDropZone(event.target)) return;
        const seq = ++sequence;
        folderDrop({ seq, pending: true });
        const dropReason = (reason) => folderDrop({ seq, reason });
        const data = event.dataTransfer;
        if (!data || data.items.length !== 1 || data.items[0].kind !== 'file' || data.files.length > 1) { dropReason('invalid_target'); return; }
        if (data.files.length === 0) { dropReason('unsupported'); return; }
        if (typeof channel.postMessageWithAdditionalObjects !== 'function') { dropReason('unsupported'); return; }
        try { channel.postMessageWithAdditionalObjects('du:folder-drop:' + seq, data.files); }
        catch { dropReason('unsupported'); }
    }, true);
    Object.defineProperty(window, '__diskunfoldRequest', { value: (payload) => new Promise((resolve, reject) => {
        const id = String(++sequence);
        const timer = setTimeout(() => {
            pending.delete(id);
            reject(new Error('DiskUnfold IPC request timed out'));
        }, 10000);
        pending.set(id, { resolve, reject, timer });
        channel.postMessage(id + '\n' + payload);
    }) });
})();
