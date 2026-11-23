// @ts-check
import init, { CycleStore } from './pkg/cycle_engine.js';

/** @typedef {import('./pkg/cycle_engine').DashboardState} DashboardState */

// ==========================================
// 1. Dumb IndexedDB Snapshot Vault (Zero Logic)
// ==========================================
const DB_NAME = 'moonphase_vault';
const STORE_NAME = 'snapshot';
const SNAPSHOT_KEY = 'current';

/** @returns {Promise<IDBDatabase>} */
function openVault() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE_NAME);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

/** @returns {Promise<string | undefined>} */
async function loadRawSnapshot() {
  const db = await openVault();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readonly');
    const req = tx.objectStore(STORE_NAME).get(SNAPSHOT_KEY);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

/** @param {string} data */
async function saveRawSnapshot(data) {
  const db = await openVault();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, 'readwrite');
    tx.objectStore(STORE_NAME).put(data, SNAPSHOT_KEY);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

// ==========================================
// 2. Helpers (Local Date & Web Share / Blob)
// ==========================================
function getLocalTodayStr() {
  const now = new Date();
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, '0');
  const d = String(now.getDate()).padStart(2, '0');
  return `${y}-${m}-${d}`;
}

/** @param {string} icsContent */
async function shareOrDownloadICS(icsContent) {
  const filename = `cycle_alerts_${getLocalTodayStr()}.ics`;
  const file = new File([icsContent], filename, { type: 'text/calendar;charset=utf-8' });

  // Prefer native share sheet on mobile if file sharing is supported
  if (navigator.canShare && navigator.canShare({ files: [file] })) {
    try {
      await navigator.share({
        files: [file],
        title: 'Cycle Calendar Alerts',
        text: 'Import into Calendar for PMS & Day 1 reminders (also contains backup).'
      });
      return;
    } catch (err) {
      // Fall back to download if share sheet fails (ignore user cancel)
      if (err instanceof Error && err.name === 'AbortError') return;
    }
  }

  // Standard download / iOS Safari Calendar preview trigger
  const url = URL.createObjectURL(file);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

// ==========================================
// 3. Bootstrap & UI Wiring
// ==========================================
async function bootstrap() {
  // Request persistent storage on Android & installed iOS PWAs
  if (navigator.storage && navigator.storage.persist) {
    navigator.storage.persist().catch(() => {});
  }

  // Register Service Worker
  if ('serviceWorker' in navigator) {
    navigator.serviceWorker.register('./sw.js').catch(console.error);
  }

  // Platform install hints
  const isIOS = /iPad|iPhone|iPod/.test(navigator.userAgent);
  const isStandalone =
    window.matchMedia('(display-mode: standalone)').matches ||
    /** @type {any} */ (window.navigator).standalone === true;

  if (isIOS && !isStandalone) {
    const hint = document.getElementById('iosInstallHint');
    if (hint) hint.style.display = 'block';
  }

  let deferredPrompt = null;
  const installBtn = /** @type {HTMLButtonElement} */ (document.getElementById('installBtn'));
  window.addEventListener('beforeinstallprompt', (e) => {
    e.preventDefault();
    deferredPrompt = e;
    installBtn.style.display = 'block';
  });

  installBtn.addEventListener('click', async () => {
    if (!deferredPrompt) return;
    deferredPrompt.prompt();
    const { outcome } = await deferredPrompt.userChoice;
    if (outcome === 'accepted') installBtn.style.display = 'none';
    deferredPrompt = null;
  });

  // Initialize Rust Wasm Store
  await init();
  const initialSnapshot = await loadRawSnapshot();
  const store = new CycleStore(initialSnapshot);

  // Set default input date to today
  const dateInput = /** @type {HTMLInputElement} */ (document.getElementById('dateInput'));
  dateInput.value = getLocalTodayStr();

  function render() {
    /** @type {DashboardState} */
    const state = store.get_dashboard_state(getLocalTodayStr());

    document.getElementById('phaseBadge').textContent = state.phase_name;
    document.getElementById('careTipText').textContent = state.care_tip;
    document.getElementById('avgCycleBadge').textContent = `Avg: ${state.avg_cycle}d`;

    const predDaysEl = document.getElementById('predDays');
    const predDetailEl = document.getElementById('predDetail');

    if (state.has_records && state.days_until_next !== undefined) {
      const days = state.days_until_next;
      predDaysEl.textContent = days >= 0 ? `In ${days} day${days === 1 ? '' : 's'}` : `${Math.abs(days)} day${Math.abs(days) === 1 ? '' : 's'} late`;
      predDetailEl.textContent = `Next: ${state.next_period_date} • PMS window: ${state.pms_start_date}`;
    } else {
      predDaysEl.textContent = '--';
      predDetailEl.textContent = 'No cycle data logged yet';
    }

    const listEl = document.getElementById('recordList');
    if (state.history.length === 0) {
      listEl.innerHTML = '<li style="color:var(--muted); justify-content:center;">No entries yet</li>';
      return;
    }

    listEl.innerHTML = state.history
      .map(
        (item) => `
        <li>
          <div>
            <strong>${item.date}</strong> <span style="color:var(--muted); font-size:0.85rem;">(${item.flow})</span>
            ${item.interval_days != null ? `<span class="pill">${item.interval_days}d cycle</span>` : ''}
            ${item.notes ? `<div class="item-meta">${escapeHtml(item.notes)}</div>` : ''}
          </div>
          <button class="danger" data-delete-id="${item.id}">Delete</button>
        </li>`
      )
      .join('');
  }

  // Event: Add Record
  document.getElementById('addForm').addEventListener('submit', async (e) => {
    e.preventDefault();
    const flowInput = /** @type {HTMLSelectElement} */ (document.getElementById('flowInput'));
    const notesInput = /** @type {HTMLInputElement} */ (document.getElementById('notesInput'));

    try {
      const snapshot = store.add_record(
        BigInt(Date.now()),
        dateInput.value,
        flowInput.value,
        notesInput.value.trim()
      );
      await saveRawSnapshot(snapshot);
      notesInput.value = '';
      render();
    } catch (err) {
      alert(String(err));
    }
  });

  // Event: Delete Record (delegated)
  document.getElementById('recordList').addEventListener('click', async (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const idAttr = target.getAttribute('data-delete-id');
    if (!idAttr) return;
    if (!confirm('Delete this entry?')) return;

    const snapshot = store.delete_record(BigInt(idAttr));
    await saveRawSnapshot(snapshot);
    render();
  });

  // Event: Export / Share .ics
  document.getElementById('exportIcsBtn').addEventListener('click', async () => {
    try {
      const ics = store.generate_ics();
      await shareOrDownloadICS(ics);
    } catch (err) {
      alert(String(err));
    }
  });

  // Event: Restore from .ics
  const icsFileInput = /** @type {HTMLInputElement} */ (document.getElementById('icsFileInput'));
  document.getElementById('importIcsBtn').addEventListener('click', () => icsFileInput.click());

  icsFileInput.addEventListener('change', async () => {
    const file = icsFileInput.files?.[0];
    if (!file) return;
    try {
      const text = await file.text();
      const snapshot = store.import_from_ics(text);
      await saveRawSnapshot(snapshot);
      render();
      alert('Cycle history restored from .ics backup!');
    } catch (err) {
      alert(String(err));
    } finally {
      icsFileInput.value = '';
    }
  });

  render();
}

/** @param {string} str */
function escapeHtml(str) {
  return str.replace(/[&<>"']/g, (ch) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch] || ch)
  );
}

bootstrap();