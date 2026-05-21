// Envoy Dashboard - Main Application

const API_BASE = '/api/dashboard';
const WS_URL = `ws://${window.location.host}/api/dashboard/stream`;

// State
let currentView = 'graph';
let stats = null;
let nodes = [];
let edges = [];
let tasks = { TODO: [], IN_PROGRESS: [], DONE: [] };
let auditEvents = [];
let ws = null;
let reconnectInterval = null;

// View switching
document.querySelectorAll('.tab').forEach(tab => {
    tab.addEventListener('click', () => {
        const view = tab.dataset.view;
        switchView(view);
    });
});

function switchView(view) {
    currentView = view;

    // Update tabs
    document.querySelectorAll('.tab').forEach(t => {
        t.classList.toggle('active', t.dataset.view === view);
    });

    // Update views
    document.querySelectorAll('.view').forEach(v => {
        v.classList.toggle('active', v.id === `${view}-view`);
    });

    // Refresh view data
    if (view === 'graph' && nodes.length === 0) {
        loadGraphData();
    } else if (view === 'kanban' && Object.values(tasks).flat().length === 0) {
        loadKanbanData();
    } else if (view === 'audit' && auditEvents.length === 0) {
        loadAuditData();
    }
}

// API client
async function fetchJSON(endpoint) {
    const response = await fetch(`${API_BASE}${endpoint}`);
    if (!response.ok) {
        throw new Error(`HTTP ${response.status}: ${response.statusText}`);
    }
    return response.json();
}

// Load statistics
async function loadStats() {
    try {
        stats = await fetchJSON('/graph/stats');
        updateStatsUI();
    } catch (error) {
        console.error('Failed to load stats:', error);
    }
}

function updateStatsUI() {
    if (!stats) return;
    document.getElementById('agent-count').textContent = `Agents: ${stats.agents}`;
    document.getElementById('discovery-count').textContent = `Discoveries: ${stats.discoveries}`;
    document.getElementById('edge-count').textContent = `Edges: ${stats.edges}`;
}

// Load graph data
async function loadGraphData() {
    try {
        const [nodesData, edgesData] = await Promise.all([
            fetchJSON('/graph/nodes'),
            fetchJSON('/graph/edges')
        ]);
        nodes = nodesData;
        edges = edgesData;
        renderGraph();
    } catch (error) {
        console.error('Failed to load graph data:', error);
        const graphDiv = document.getElementById('graph');
        graphDiv.textContent = 'Failed to load graph data';
        graphDiv.style.display = 'flex';
        graphDiv.style.alignItems = 'center';
        graphDiv.style.justifyContent = 'center';
        graphDiv.style.height = '100%';
        graphDiv.style.color = 'var(--text-secondary)';
    }
}

// Load kanban data
async function loadKanbanData() {
    try {
        const data = await fetchJSON('/tasks');
        tasks = {
            TODO: data.TODO || [],
            IN_PROGRESS: data.IN_PROGRESS || [],
            DONE: data.DONE || []
        };
        renderKanban();
    } catch (error) {
        console.error('Failed to load tasks:', error);
    }
}

// Load audit data
async function loadAuditData() {
    try {
        auditEvents = await fetchJSON('/audit?limit=50');
        renderAudit();
    } catch (error) {
        console.error('Failed to load audit data:', error);
    }
}

// WebSocket connection
function connectWebSocket() {
    ws = new WebSocket(WS_URL);

    ws.onopen = () => {
        console.log('Dashboard WebSocket connected');

        // Clear any reconnect interval
        if (reconnectInterval) {
            clearInterval(reconnectInterval);
            reconnectInterval = null;
        }
    };

    ws.onmessage = (event) => {
        try {
            const message = JSON.parse(event.data);
            handleWebSocketMessage(message);
        } catch (error) {
            console.error('Failed to parse WebSocket message:', error);
        }
    };

    ws.onclose = () => {
        console.log('Dashboard WebSocket disconnected, reconnecting in 5s...');

        // Attempt to reconnect after 5 seconds
        if (!reconnectInterval) {
            reconnectInterval = setInterval(() => {
                connectWebSocket();
            }, 5000);
        }
    };

    ws.onerror = (error) => {
        console.error('WebSocket error:', error);
    };
}

function handleWebSocketMessage(message) {
    switch (message.event) {
        case 'connected':
            console.log('WebSocket confirmed:', message.data.message);
            break;

        case 'stats_update':
            stats = message.data;
            updateStatsUI();
            break;

        case 'graph_update':
            // Refresh graph data on update
            if (currentView === 'graph') {
                loadGraphData();
            }
            break;

        case 'task_update':
            // Refresh kanban data on update
            if (currentView === 'kanban') {
                loadKanbanData();
            }
            break;

        case 'audit_event':
            // Add new audit event and refresh
            if (currentView === 'audit') {
                loadAuditData();
            }
            break;

        default:
            console.log('Unknown WebSocket event:', message.event);
    }
}

// Initialize
async function init() {
    await loadStats();
    await loadGraphData();

    // Connect WebSocket for live updates
    connectWebSocket();
}

// Start the app
init();

// Cleanup on page unload
window.addEventListener('beforeunload', () => {
    if (ws) {
        ws.close();
    }
    if (reconnectInterval) {
        clearInterval(reconnectInterval);
    }
});
