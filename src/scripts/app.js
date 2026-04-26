// ═══════════════════════════════════════════════════════
// SidePar — Main Application Logic
// ═══════════════════════════════════════════════════════

const { invoke } = window.__TAURI__.core;
const { listen, emit } = window.__TAURI__.event;
const { writeText } = window.__TAURI_PLUGIN_CLIPBOARD_MANAGER__;

// ─── State ──────────────────────────────────────────────

let currentTab = 'history';
let searchQuery = '';
let clipboardHistory = [];
let prompts = [];
let categories = [];
let selectedCategory = 'all';
let editingPromptId = null;
let shouldAnimate = false;

// ─── DOM Elements ───────────────────────────────────────

const panel = document.getElementById('panel-container');
const searchInput = document.getElementById('search-input');
const tabHistory = document.getElementById('tab-history');
const tabPrompts = document.getElementById('tab-prompts');
const contentHistory = document.getElementById('content-history');
const contentPrompts = document.getElementById('content-prompts');
const historyList = document.getElementById('history-list');
const promptsList = document.getElementById('prompts-list');
const categoryPills = document.getElementById('category-pills');

// Modals
const modalOverlay = document.getElementById('modal-overlay');
const modalTitle = document.getElementById('modal-title');
const promptTitleInput = document.getElementById('prompt-title');
const promptCategorySelect = document.getElementById('prompt-category');
const promptContentInput = document.getElementById('prompt-content');
const catModalOverlay = document.getElementById('category-modal-overlay');

// ─── Toast ──────────────────────────────────────────────

function showToast(msg) {
  let toast = document.querySelector('.toast');
  if (!toast) {
    toast = document.createElement('div');
    toast.className = 'toast';
    document.body.appendChild(toast);
  }
  toast.textContent = msg;
  toast.classList.add('show');
  setTimeout(() => toast.classList.remove('show'), 1500);
}

// ─── Panel Show/Hide ────────────────────────────────────

listen('panel-show', async () => {
  // Reload all data from Rust/SQLite state
  shouldAnimate = true;
  await loadCategories();
  await loadHistory();
  await loadPrompts();
  shouldAnimate = false;
  requestAnimationFrame(() => {
    panel.classList.add('active');
    searchInput.focus();
  });
});

listen('panel-hide', () => {
  panel.classList.remove('active');
});

// ─── Tab Switching ──────────────────────────────────────

function switchTab(tab) {
  currentTab = tab;
  tabHistory.classList.toggle('active', tab === 'history');
  tabPrompts.classList.toggle('active', tab === 'prompts');
  contentHistory.classList.toggle('active', tab === 'history');
  contentPrompts.classList.toggle('active', tab === 'prompts');
  searchInput.placeholder = tab === 'history' ? '搜索历史记录...' : '搜索提示词...';
  filterAndRender();
}

tabHistory.addEventListener('click', () => switchTab('history'));
tabPrompts.addEventListener('click', () => switchTab('prompts'));

// ─── Search ─────────────────────────────────────────────

searchInput.addEventListener('input', (e) => {
  searchQuery = e.target.value.toLowerCase().trim();
  filterAndRender();
});

function filterAndRender() {
  if (currentTab === 'history') renderHistory();
  else renderPrompts();
}

// ─── Time Formatting ────────────────────────────────────

function formatTime(ts) {
  const d = new Date(ts);
  const now = new Date();
  const diff = now - d;
  if (diff < 60000) return '刚刚';
  if (diff < 3600000) return `${Math.floor(diff / 60000)} 分钟前`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)} 小时前`;
  if (diff < 172800000) return '昨天';
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
}

// ─── Clipboard History ──────────────────────────────────

async function loadHistory() {
  clipboardHistory = await invoke('get_clipboard_history');
  renderHistory();
}

function renderHistory() {
  let items = clipboardHistory;
  if (searchQuery) {
    items = items.filter(i => i.content.toLowerCase().includes(searchQuery));
  }
  // Sort: pinned first, then by timestamp
  items.sort((a, b) => {
    if (a.pinned !== b.pinned) return b.pinned ? 1 : -1;
    return b.timestamp - a.timestamp;
  });

  if (items.length === 0) {
    historyList.innerHTML = `
      <div class="empty-state">
        <div class="empty-icon">${searchQuery ? '🔍' : '📋'}</div>
        <p>${searchQuery ? '未找到匹配记录' : '暂无剪贴板记录'}</p>
        <p class="empty-hint">${searchQuery ? '尝试其他关键词' : '复制任意文本后将自动出现在这里'}</p>
      </div>`;
    return;
  }

  historyList.innerHTML = items.map((item, i) => `
    <div class="history-item ${item.pinned ? 'pinned' : ''} ${shouldAnimate ? 'animate-in' : ''}" data-id="${item.id}" style="${shouldAnimate ? 'animation-delay:' + (i * 0.03) + 's' : ''}">
      <div class="item-content">${escapeHtml(item.preview)}</div>
      <div class="item-meta">
        <span class="item-time">${formatTime(item.timestamp)}</span>
        <div class="item-actions">
          <button class="item-action-btn" onclick="event.stopPropagation(); togglePin('${item.id}')" title="${item.pinned ? '取消置顶' : '置顶'}">
            ${item.pinned ? '★' : '☆'}
          </button>
          <button class="item-action-btn danger" onclick="event.stopPropagation(); deleteHistoryItem('${item.id}')" title="删除">✕</button>
        </div>
      </div>
    </div>
  `).join('');

  // Click to copy
  historyList.querySelectorAll('.history-item').forEach(el => {
    el.addEventListener('click', () => copyHistoryItem(el.dataset.id));
  });
}

async function copyHistoryItem(id) {
  const item = clipboardHistory.find(i => i.id === id);
  if (!item) return;
  await writeText(item.content);
  await invoke('copy_to_clipboard', { text: item.content });
  const el = historyList.querySelector(`[data-id="${id}"]`);
  if (el) {
    el.classList.add('copied');
    setTimeout(() => el.classList.remove('copied'), 600);
  }
  showToast('已复制');
}

window.togglePin = async (id) => {
  await invoke('toggle_pin_item', { id });
  await loadHistory();
};

window.deleteHistoryItem = async (id) => {
  await invoke('delete_history_item', { id });
  clipboardHistory = clipboardHistory.filter(i => i.id !== id);
  renderHistory();
};

// Listen for new clipboard items
listen('clipboard-changed', (event) => {
  const newItem = event.payload;
  // Remove duplicate
  clipboardHistory = clipboardHistory.filter(i => i.content !== newItem.content);
  clipboardHistory.unshift(newItem);
  if (clipboardHistory.length > 50) clipboardHistory.length = 50;
  if (currentTab === 'history') renderHistory();
});

// Clear all
document.getElementById('btn-clear-history').addEventListener('click', async () => {
  await invoke('clear_clipboard_history');
  clipboardHistory = [];
  renderHistory();
  showToast('历史已清空');
});

// ─── Prompts ────────────────────────────────────────────

async function loadPrompts() {
  const data = await invoke('get_prompts');
  prompts = Array.isArray(data) ? data : [];
  renderPrompts();
}

async function loadCategories() {
  const data = await invoke('get_categories');
  categories = Array.isArray(data) ? data : [];
  renderCategoryPills();
  renderCategorySelect();
}

function renderCategoryPills() {
  let html = '<button class="category-pill ' + (selectedCategory === 'all' ? 'active' : '') + '" data-category="all">全部</button>';
  categories.forEach(cat => {
    const isActive = selectedCategory === cat.id ? 'active' : '';
    html += `<button class="category-pill ${isActive}" data-category="${cat.id}" style="${isActive ? 'background:' + cat.color + ';border-color:' + cat.color : ''}">${escapeHtml(cat.name)}</button>`;
  });
  categoryPills.innerHTML = html;

  categoryPills.querySelectorAll('.category-pill').forEach(btn => {
    btn.addEventListener('click', () => {
      selectedCategory = btn.dataset.category;
      renderCategoryPills();
      renderPrompts();
    });
  });
}

function renderCategorySelect() {
  let html = '<option value="">未分类</option>';
  categories.forEach(cat => {
    html += `<option value="${cat.id}">${escapeHtml(cat.name)}</option>`;
  });
  promptCategorySelect.innerHTML = html;
}

function renderPrompts() {
  let items = prompts;
  if (selectedCategory !== 'all') {
    items = items.filter(p => p.categoryId === selectedCategory);
  }
  if (searchQuery) {
    items = items.filter(p =>
      p.title.toLowerCase().includes(searchQuery) ||
      p.content.toLowerCase().includes(searchQuery)
    );
  }

  if (items.length === 0) {
    promptsList.innerHTML = `
      <div class="empty-state">
        <div class="empty-icon">${searchQuery ? '🔍' : '✨'}</div>
        <p>${searchQuery ? '未找到匹配提示词' : '暂无提示词'}</p>
        <p class="empty-hint">${searchQuery ? '尝试其他关键词' : '点击下方按钮添加常用提示词'}</p>
      </div>`;
    return;
  }

  promptsList.innerHTML = items.map((p, i) => {
    const cat = categories.find(c => c.id === p.categoryId);
    const catBadge = cat ? `<span class="prompt-category-badge" style="background:${cat.color}">${escapeHtml(cat.name)}</span>` : '';
    return `
    <div class="prompt-item ${shouldAnimate ? 'animate-in' : ''}" data-id="${p.id}" style="${shouldAnimate ? 'animation-delay:' + (i * 0.03) + 's' : ''}">
      <div class="prompt-header">
        <span class="prompt-title">${escapeHtml(p.title)}</span>
        ${catBadge}
      </div>
      <div class="prompt-preview">${escapeHtml(p.content)}</div>
      <div class="item-meta">
        <div class="item-actions">
          <button class="item-action-btn" onclick="event.stopPropagation(); editPrompt('${p.id}')" title="编辑">✎</button>
          <button class="item-action-btn danger" onclick="event.stopPropagation(); deletePromptItem('${p.id}')" title="删除">✕</button>
        </div>
      </div>
    </div>`;
  }).join('');

  promptsList.querySelectorAll('.prompt-item').forEach(el => {
    el.addEventListener('click', () => copyPrompt(el.dataset.id));
  });
}

async function copyPrompt(id) {
  const p = prompts.find(i => i.id === id);
  if (!p) return;
  await writeText(p.content);
  await invoke('copy_to_clipboard', { text: p.content });
  const el = promptsList.querySelector(`[data-id="${id}"]`);
  if (el) {
    el.classList.add('copied');
    setTimeout(() => el.classList.remove('copied'), 600);
  }
  showToast('已复制');
}

window.editPrompt = (id) => {
  const p = prompts.find(i => i.id === id);
  if (!p) return;
  editingPromptId = id;
  modalTitle.textContent = '编辑提示词';
  promptTitleInput.value = p.title;
  promptCategorySelect.value = p.categoryId || '';
  promptContentInput.value = p.content;
  modalOverlay.classList.add('active');
};

window.deletePromptItem = async (id) => {
  await invoke('delete_prompt', { id });
  prompts = prompts.filter(p => p.id !== id);
  renderPrompts();
  showToast('已删除');
};

// ─── Prompt Modal ───────────────────────────────────────

document.getElementById('btn-add-prompt').addEventListener('click', () => {
  editingPromptId = null;
  modalTitle.textContent = '新建提示词';
  promptTitleInput.value = '';
  promptCategorySelect.value = '';
  promptContentInput.value = '';
  modalOverlay.classList.add('active');
  promptTitleInput.focus();
});

document.getElementById('btn-close-modal').addEventListener('click', () => {
  modalOverlay.classList.remove('active');
});

document.getElementById('btn-cancel-modal').addEventListener('click', () => {
  modalOverlay.classList.remove('active');
});

document.getElementById('btn-save-prompt').addEventListener('click', async () => {
  const title = promptTitleInput.value.trim();
  const content = promptContentInput.value.trim();
  if (!title || !content) {
    showToast('请填写标题和内容');
    return;
  }

  const now = Date.now();
  const prompt = {
    id: editingPromptId || crypto.randomUUID(),
    title,
    content,
    categoryId: promptCategorySelect.value,
    createdAt: editingPromptId ? (prompts.find(p => p.id === editingPromptId)?.createdAt || now) : now,
    updatedAt: now,
  };

  await invoke('save_prompt', { prompt });

  if (editingPromptId) {
    const idx = prompts.findIndex(p => p.id === editingPromptId);
    if (idx !== -1) prompts[idx] = prompt;
  } else {
    prompts.push(prompt);
  }

  modalOverlay.classList.remove('active');
  renderPrompts();
  showToast(editingPromptId ? '已更新' : '已创建');
});

// ─── Category Modal ─────────────────────────────────────

document.getElementById('btn-manage-categories').addEventListener('click', () => {
  renderCategoryList();
  catModalOverlay.classList.add('active');
});

document.getElementById('btn-close-cat-modal').addEventListener('click', () => {
  catModalOverlay.classList.remove('active');
});

function renderCategoryList() {
  const list = document.getElementById('category-list');
  if (categories.length === 0) {
    list.innerHTML = '<p style="color: var(--text-tertiary); font-size: 13px; padding: 8px;">暂无分类</p>';
    return;
  }
  list.innerHTML = categories.map(cat => `
    <div class="category-manage-item">
      <div class="category-color-dot" style="background: ${cat.color}"></div>
      <span class="cat-name">${escapeHtml(cat.name)}</span>
      <button class="cat-delete" onclick="deleteCategoryItem('${cat.id}')">✕</button>
    </div>
  `).join('');
}

document.getElementById('btn-add-category').addEventListener('click', async () => {
  const name = document.getElementById('new-category-name').value.trim();
  const color = document.getElementById('new-category-color').value;
  if (!name) return;

  const cat = {
    id: crypto.randomUUID(),
    name,
    color,
    order: categories.length,
  };

  await invoke('save_category', { category: cat });
  categories.push(cat);
  document.getElementById('new-category-name').value = '';
  renderCategoryList();
  renderCategoryPills();
  renderCategorySelect();
  showToast('分类已添加');
});

window.deleteCategoryItem = async (id) => {
  await invoke('delete_category', { id });
  categories = categories.filter(c => c.id !== id);
  if (selectedCategory === id) selectedCategory = 'all';
  renderCategoryList();
  renderCategoryPills();
  renderCategorySelect();
};

// ─── Utilities ──────────────────────────────────────────

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// ─── Initialize ─────────────────────────────────────────

async function init() {
  await loadHistory();
  await loadCategories();
  await loadPrompts();
}

init();
