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
// 2. Native <dialog> Helpers (取代 alert / confirm)
// ==========================================
/**
 * @param {string} message
 * @param {boolean} [isConfirm]
 * @returns {Promise<boolean>}
 */
function showModal(message, isConfirm = false) {
  const dialog = /** @type {HTMLDialogElement} */ (document.getElementById('appDialog'));
  const msgEl = document.getElementById('dialogMessage');
  const cancelBtn = /** @type {HTMLButtonElement} */ (document.getElementById('dialogCancelBtn'));
  const confirmBtn = /** @type {HTMLButtonElement} */ (document.getElementById('dialogConfirmBtn'));

  msgEl.textContent = message;
  cancelBtn.style.display = isConfirm ? 'inline-block' : 'none';

  return new Promise((resolve) => {
    const cleanup = (result) => {
      dialog.close();
      confirmBtn.removeEventListener('click', onConfirm);
      cancelBtn.removeEventListener('click', onCancel);
      resolve(result);
    };
    const onConfirm = () => cleanup(true);
    const onCancel = () => cleanup(false);

    confirmBtn.addEventListener('click', onConfirm);
    cancelBtn.addEventListener('click', onCancel);
    dialog.showModal();
  });
}

// ==========================================
// 3. Date & Web Share Helpers
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

  if (navigator.canShare && navigator.canShare({ files: [file] })) {
    try {
      await navigator.share({
        files: [file],
        title: '週期行事曆提醒與備份',
        text: '匯入行事曆即可設定 PMS 與經期通知（內含完整歷史備份）。'
      });
      return;
    } catch (err) {
      if (err instanceof Error && err.name === 'AbortError') return;
    }
  }

  const url = URL.createObjectURL(file);
  const a = document.createElement('a');
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  document.body.removeChild(a);
  URL.revokeObjectURL(url);
}

/** @param {string} str */
function escapeHtml(str) {
  return str.replace(/[&<>"']/g, (ch) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[ch] || ch)
  );
}

// ==========================================
// 4. Bootstrap & UI Wiring
// ==========================================
async function bootstrap() {
  if (navigator.storage && navigator.storage.persist) {
    navigator.storage.persist().catch(() => {});
  }

  if ('serviceWorker' in navigator) {
    let refreshing = false;
    navigator.serviceWorker.addEventListener('controllerchange', () => {
      if (refreshing) return;
      refreshing = true;
      window.location.reload();
    });

    navigator.serviceWorker.register('./sw.js', { updateViaCache: 'none' }).then((reg) => {
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible') reg.update().catch(() => {});
      });
    }).catch(console.error);
  }

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

  await init();
  const store = new CycleStore(await loadRawSnapshot());

  const addForm = /** @type {HTMLFormElement} */ (document.getElementById('addForm'));
  const dateInput = /** @type {HTMLInputElement} */ (document.getElementById('dateInput'));
  dateInput.value = getLocalTodayStr();

  const now = new Date();
  let viewYear = now.getFullYear();
  let viewMonth = now.getMonth() + 1;

  function render() {
    /** @type {DashboardState} */
    const state = store.get_dashboard_state(getLocalTodayStr(), viewYear, viewMonth);

    document.getElementById('phaseBadge').textContent = state.phase_name;
    document.getElementById('careTipText').textContent = state.care_tip;
    document.getElementById('avgCycleBadge').textContent =
      `週期 ${state.avg_cycle}d ｜ 經期 ${state.avg_duration}d`;

    const countEl = document.getElementById('historyCount');
    if (countEl) countEl.textContent = `(${state.history.length})`;

    const predDaysEl = document.getElementById('predDays');
    const predDetailEl = document.getElementById('predDetail');

    if (state.has_records && state.days_until_next !== undefined) {
      const days = state.days_until_next;
      predDaysEl.textContent =
        days >= 0 ? `距離下次還有 ${days} 天` : `預計經期已延遲 ${Math.abs(days)} 天`;
      predDetailEl.textContent = `預計下次：${state.next_period_date} ｜ PMS 觀察：${state.pms_start_date}`;
    } else {
      predDaysEl.textContent = '-- 天';
      predDetailEl.textContent = '尚未建立任何週期紀錄';
    }

    document.getElementById('calMonthTitle').textContent =
      `${state.calendar_year} 年 ${state.calendar_month} 月`;

    document.getElementById('calendarGrid').innerHTML = state.calendar_days
      .map((cell) => {
        if (cell.day == null) return '<div class="cal-cell empty"></div>';
        const classes = ['cal-cell'];
        if (cell.status !== 'none') classes.push(cell.status);
        if (cell.is_today) classes.push('today');
        return `<div class="${classes.join(' ')}" data-date="${cell.date_str}">${cell.day}</div>`;
      })
      .join('');

    const listEl = document.getElementById('recordList');
    if (state.history.length === 0) {
      listEl.innerHTML = '<li style="color:var(--muted); justify-content:center;">尚無紀錄</li>';
      return;
    }

    const flowMap = { Normal: '正常', Heavy: '偏多', Light: '偏少' };
    const durationChoices = [3, 4, 5, 6, 7, 8];

    listEl.innerHTML = state.history
      .map((item) => {
        const displayFlow = flowMap[item.flow] || item.flow;
        const optionsHtml = [
          `<option value="" ${item.duration_days == null ? 'selected' : ''}>預估(${item.effective_duration}天)</option>`,
          ...durationChoices.map(
            (d) => `<option value="${d}" ${item.duration_days === d ? 'selected' : ''}>${d} 天</option>`
          )
        ].join('');

        return `
        <li>
          <div style="min-width:0; flex:1;">
            <strong>${item.date}</strong> <span style="color:var(--muted); font-size:0.83rem;">(${escapeHtml(displayFlow)})</span>
            ${item.interval_days != null ? `<span class="pill">間隔 ${item.interval_days}d</span>` : ''}
            ${item.notes ? `<div class="item-meta">${escapeHtml(item.notes)}</div>` : ''}
          </div>
          <div class="history-actions">
            <select class="mini-select" data-duration-id="${item.id}" aria-label="行經天數">
              ${optionsHtml}
            </select>
            <button type="button" class="danger" data-delete-id="${item.id}">刪除</button>
          </div>
        </li>`;
      })
      .join('');
  }

  // 月份切換與「今天」一鍵跳回
  document.getElementById('prevMonthBtn').addEventListener('click', () => {
    viewMonth -= 1;
    if (viewMonth < 1) {
      viewMonth = 12;
      viewYear -= 1;
    }
    render();
  });

  document.getElementById('nextMonthBtn').addEventListener('click', () => {
    viewMonth += 1;
    if (viewMonth > 12) {
      viewMonth = 1;
      viewYear += 1;
    }
    render();
  });

  document.getElementById('todayBtn').addEventListener('click', () => {
    const today = new Date();
    viewYear = today.getFullYear();
    viewMonth = today.getMonth() + 1;
    dateInput.value = getLocalTodayStr();
    render();
  });

  // 點擊月曆日期自動填入表單
  document.getElementById('calendarGrid').addEventListener('click', (e) => {
    const cell = /** @type {HTMLElement} */ (e.target).closest('[data-date]');
    const pickedDate = cell?.getAttribute('data-date');
    if (pickedDate) {
      dateInput.value = pickedDate;
      dateInput.scrollIntoView({ behavior: 'smooth', block: 'center' });
    }
  });

  // 新增紀錄（透過原生 FormData 自動彙整勾選的 Checkbox 標籤與文字備註）
  addForm.addEventListener('submit', async (e) => {
    e.preventDefault();
    const formData = new FormData(addForm);
    const dateVal = String(formData.get('date') || '');
    const flowVal = String(formData.get('flow') || '正常');
    const rawDur = String(formData.get('duration') || '');
    const durVal = rawDur ? Number(rawDur) : undefined;

    const checkedSymptoms = formData.getAll('symptom').map(String);
    const extraNote = String(formData.get('notes') || '').trim();
    if (extraNote) checkedSymptoms.push(extraNote);
    const combinedNotes = checkedSymptoms.join('、');

    try {
      const snapshot = store.add_record(
        BigInt(Date.now()),
        dateVal,
        flowVal,
        combinedNotes,
        durVal
      );
      await saveRawSnapshot(snapshot);
      addForm.reset();
      dateInput.value = getLocalTodayStr();
      render();
    } catch (err) {
      await showModal(String(err));
    }
  });

  // 歷史列表：一鍵修改單筆行經天數
  document.getElementById('recordList').addEventListener('change', async (e) => {
    const target = /** @type {HTMLSelectElement} */ (e.target);
    const idAttr = target.getAttribute('data-duration-id');
    if (!idAttr) return;

    const durVal = target.value ? Number(target.value) : undefined;
    try {
      const snapshot = store.update_duration(BigInt(idAttr), durVal);
      await saveRawSnapshot(snapshot);
      render();
    } catch (err) {
      await showModal(String(err));
    }
  });

  // 歷史列表：原生 <dialog> 刪除確認
  document.getElementById('recordList').addEventListener('click', async (e) => {
    const target = /** @type {HTMLElement} */ (e.target);
    const idAttr = target.getAttribute('data-delete-id');
    if (!idAttr) return;

    const ok = await showModal('確定要刪除此筆紀錄嗎？', true);
    if (!ok) return;

    const snapshot = store.delete_record(BigInt(idAttr));
    await saveRawSnapshot(snapshot);
    render();
  });

  // 匯出 / 分享 .ics
  document.getElementById('exportIcsBtn').addEventListener('click', async () => {
    try {
      const ics = store.generate_ics();
      await shareOrDownloadICS(ics);
    } catch (err) {
      await showModal(String(err));
    }
  });

  // 從 .ics 還原
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
      await showModal('已成功從 .ics 檔案還原歷史紀錄！');
    } catch (err) {
      await showModal(String(err));
    } finally {
      icsFileInput.value = '';
    }
  });

  render();
}

bootstrap();