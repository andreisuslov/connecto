// Connecto GUI - Frontend Application

const { invoke } = window.__TAURI__.tauri;

// State
let isScanning = false;
let isListening = false;
let devices = [];

// DOM Elements
const tabs = document.querySelectorAll('.tab');
const tabContents = document.querySelectorAll('.tab-content');

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    setupTabs();
    setupScanTab();
    setupListenTab();
    setupKeysTab();

    // Load initial data
    await loadDeviceName();
    await loadAddresses();
    await loadKeys();
});

// Tab Navigation
function setupTabs() {
    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            const targetId = tab.dataset.tab;

            tabs.forEach(t => t.classList.remove('active'));
            tabContents.forEach(c => c.classList.remove('active'));

            tab.classList.add('active');
            document.getElementById(targetId).classList.add('active');
        });
    });
}

// Scan Tab
function setupScanTab() {
    const scanBtn = document.getElementById('scanBtn');
    const scanTimeout = document.getElementById('scanTimeout');

    scanBtn.addEventListener('click', async () => {
        if (isScanning) return;

        const timeout = parseInt(scanTimeout.value);
        await scanNetwork(timeout);
    });
}

async function scanNetwork(timeout) {
    const scanBtn = document.getElementById('scanBtn');
    const deviceList = document.getElementById('deviceList');
    const scanStatus = document.getElementById('scanStatus');

    isScanning = true;
    scanBtn.disabled = true;
    scanBtn.innerHTML = '<span class="spinner"></span> Scanning...';

    showStatus(scanStatus, 'info', `Scanning for ${timeout} seconds...`);

    try {
        devices = await invoke('scan_devices', { timeoutSecs: timeout });

        if (devices.length === 0) {
            deviceList.innerHTML = `
                <p class="empty-state">
                    No devices found.<br>
                    <small>Make sure the target device is running "connecto listen"</small>
                </p>
            `;
            showStatus(scanStatus, 'warning', 'No devices found on the network.');
        } else {
            deviceList.innerHTML = devices.map((device, index) => `
                <div class="device-item">
                    <div class="device-info">
                        <div class="device-name">${escapeHtml(extractFriendlyName(device.name))}</div>
                        <div class="device-address">${device.addresses[0] || 'Unknown'}:${device.port}</div>
                    </div>
                    <button class="btn primary" onclick="pairWithDevice(${index})">
                        Pair
                    </button>
                </div>
            `).join('');
            showStatus(scanStatus, 'success', `Found ${devices.length} device(s).`);
        }
    } catch (error) {
        showStatus(scanStatus, 'error', `Scan failed: ${error}`);
    } finally {
        isScanning = false;
        scanBtn.disabled = false;
        scanBtn.innerHTML = '<span class="icon">🔍</span> Scan Network';
    }
}

async function pairWithDevice(index) {
    const pairResult = document.getElementById('pairResult');
    const resultContent = document.getElementById('pairResultContent');

    showStatus(document.getElementById('scanStatus'), 'info', 'Pairing...');

    try {
        const result = await invoke('pair_with_device', {
            deviceIndex: index,
            useRsa: false,
            customComment: null
        });

        pairResult.classList.remove('hidden');

        if (result.success) {
            resultContent.innerHTML = `
                <p class="result-success">Successfully paired with ${escapeHtml(result.server_name)}!</p>
                <div class="result-item">
                    <label>SSH Command:</label>
                    <code>${escapeHtml(result.ssh_command)}</code>
                </div>
                <div class="result-item">
                    <label>Private Key:</label>
                    <code>${escapeHtml(result.private_key_path)}</code>
                </div>
                <div class="result-item">
                    <label>Public Key:</label>
                    <code>${escapeHtml(result.public_key_path)}</code>
                </div>
            `;
            showStatus(document.getElementById('scanStatus'), 'success', 'Pairing successful!');
        } else {
            resultContent.innerHTML = `
                <p class="result-error">Pairing failed: ${escapeHtml(result.error || 'Unknown error')}</p>
            `;
            showStatus(document.getElementById('scanStatus'), 'error', `Pairing failed: ${result.error}`);
        }
    } catch (error) {
        pairResult.classList.remove('hidden');
        resultContent.innerHTML = `
            <p class="result-error">Error: ${escapeHtml(error.toString())}</p>
        `;
        showStatus(document.getElementById('scanStatus'), 'error', `Error: ${error}`);
    }
}

// Listen Tab
function setupListenTab() {
    const startBtn = document.getElementById('startListenBtn');
    const stopBtn = document.getElementById('stopListenBtn');

    startBtn.addEventListener('click', startListening);
    stopBtn.addEventListener('click', stopListening);
}

async function loadDeviceName() {
    try {
        const name = await invoke('get_device_name');
        document.getElementById('deviceName').placeholder = name;
    } catch (error) {
        console.error('Failed to get device name:', error);
    }
}

async function loadAddresses() {
    try {
        const addresses = await invoke('get_addresses');
        const container = document.getElementById('listenAddresses');

        if (addresses.length > 0) {
            container.innerHTML = `
                <h3>Your IP Addresses:</h3>
                <div class="address-list">
                    ${addresses.map(addr => `<span class="address-tag">${escapeHtml(addr)}</span>`).join('')}
                </div>
            `;
        }
    } catch (error) {
        console.error('Failed to get addresses:', error);
    }
}

async function startListening() {
    const startBtn = document.getElementById('startListenBtn');
    const stopBtn = document.getElementById('stopListenBtn');
    const listenStatus = document.getElementById('listenStatus');
    const port = parseInt(document.getElementById('listenPort').value) || 8099;
    const deviceName = document.getElementById('deviceName').value || null;

    startBtn.disabled = true;
    startBtn.innerHTML = '<span class="spinner"></span> Starting...';

    try {
        const status = await invoke('start_listener', {
            port,
            deviceName
        });

        isListening = true;
        startBtn.classList.add('hidden');
        stopBtn.classList.remove('hidden');

        showStatus(listenStatus, 'success',
            `Listening on port ${status.port}. Other devices can now pair with "${status.device_name}".`
        );
    } catch (error) {
        showStatus(listenStatus, 'error', `Failed to start listener: ${error}`);
    } finally {
        startBtn.disabled = false;
        startBtn.innerHTML = '<span class="icon">📡</span> Start Listening';
    }
}

async function stopListening() {
    const startBtn = document.getElementById('startListenBtn');
    const stopBtn = document.getElementById('stopListenBtn');
    const listenStatus = document.getElementById('listenStatus');

    stopBtn.disabled = true;

    try {
        await invoke('stop_listener');

        isListening = false;
        stopBtn.classList.add('hidden');
        startBtn.classList.remove('hidden');

        showStatus(listenStatus, 'info', 'Listener stopped.');
    } catch (error) {
        showStatus(listenStatus, 'error', `Failed to stop listener: ${error}`);
    } finally {
        stopBtn.disabled = false;
    }
}

// Keys Tab
function setupKeysTab() {
    const refreshBtn = document.getElementById('refreshKeysBtn');
    const generateBtn = document.getElementById('generateKeyBtn');

    refreshBtn.addEventListener('click', loadKeys);
    generateBtn.addEventListener('click', generateKeyPair);
}

async function loadKeys() {
    const keysList = document.getElementById('keysList');

    try {
        const keys = await invoke('list_authorized_keys');

        if (keys.length === 0) {
            keysList.innerHTML = '<p class="empty-state">No authorized keys found.</p>';
        } else {
            keysList.innerHTML = keys.map((key, index) => {
                const parts = key.split(/\s+/);
                const keyType = parts[0] || 'unknown';
                const keyData = parts[1] || '';
                const comment = parts.slice(2).join(' ') || 'No comment';
                const preview = keyData.length > 40
                    ? `${keyData.substring(0, 20)}...${keyData.substring(keyData.length - 20)}`
                    : keyData;

                return `
                    <div class="key-item">
                        <div class="key-info">
                            <span class="key-type">${escapeHtml(keyType)}</span>
                            <div class="key-comment">${escapeHtml(comment)}</div>
                            <div class="key-preview">${escapeHtml(preview)}</div>
                        </div>
                        <button class="btn danger" onclick="removeKey(${index}, \`${escapeHtml(key).replace(/`/g, '\\`')}\`)">
                            Remove
                        </button>
                    </div>
                `;
            }).join('');
        }
    } catch (error) {
        keysList.innerHTML = `<p class="empty-state">Error loading keys: ${escapeHtml(error.toString())}</p>`;
    }
}

async function removeKey(index, key) {
    if (!confirm('Are you sure you want to remove this key?')) return;

    try {
        await invoke('remove_authorized_key', { key });
        await loadKeys();
    } catch (error) {
        alert(`Failed to remove key: ${error}`);
    }
}

async function generateKeyPair() {
    const nameInput = document.getElementById('keygenName');
    const commentInput = document.getElementById('keygenComment');
    const rsaCheckbox = document.getElementById('keygenRsa');
    const resultDiv = document.getElementById('keygenResult');
    const generateBtn = document.getElementById('generateKeyBtn');

    const name = nameInput.value.trim() || 'connecto_key';
    const comment = commentInput.value.trim() || null;
    const useRsa = rsaCheckbox.checked;

    generateBtn.disabled = true;
    generateBtn.innerHTML = '<span class="spinner"></span> Generating...';

    try {
        const [privatePath, publicPath] = await invoke('generate_key_pair', {
            name,
            comment,
            useRsa
        });

        resultDiv.classList.remove('hidden');
        resultDiv.innerHTML = `
            <h4>Key Generated Successfully!</h4>
            <code>Private: ${escapeHtml(privatePath)}</code>
            <code>Public: ${escapeHtml(publicPath)}</code>
        `;

        // Clear inputs
        nameInput.value = '';
        commentInput.value = '';
        rsaCheckbox.checked = false;
    } catch (error) {
        resultDiv.classList.remove('hidden');
        resultDiv.innerHTML = `<h4 style="color: var(--danger);">Error: ${escapeHtml(error.toString())}</h4>`;
    } finally {
        generateBtn.disabled = false;
        generateBtn.innerHTML = '<span class="icon">🔑</span> Generate Key Pair';
    }
}

// Utility Functions
function showStatus(element, type, message) {
    element.className = `status ${type}`;
    element.textContent = message;
    element.classList.remove('hidden');
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function extractFriendlyName(fullName) {
    return fullName.split('._connecto')[0];
}

// Make functions available globally for onclick handlers
window.pairWithDevice = pairWithDevice;
window.removeKey = removeKey;
