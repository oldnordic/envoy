// Envoy Dashboard - Audit Timeline View

function renderAudit() {
    const container = document.getElementById('audit-timeline');
    // Clear existing events
    while (container.firstChild) {
        container.removeChild(container.firstChild);
    }

    if (auditEvents.length === 0) {
        const emptyMsg = document.createElement('div');
        emptyMsg.textContent = 'No audit events';
        emptyMsg.style.cssText = 'color:var(--text-secondary);text-align:center;padding:40px;';
        container.appendChild(emptyMsg);
        return;
    }

    auditEvents.forEach(event => {
        const eventDiv = createAuditEvent(event);
        container.appendChild(eventDiv);
    });
}

function createAuditEvent(event) {
    const div = document.createElement('div');
    div.className = 'audit-event';

    const timestamp = document.createElement('div');
    timestamp.className = 'timestamp';
    timestamp.textContent = new Date(event.timestamp).toLocaleString();

    const eventType = document.createElement('div');
    eventType.className = 'event-type';
    eventType.textContent = event.event_type;

    const agent = document.createElement('div');
    agent.className = 'agent';
    agent.textContent = `Agent: ${event.agent}`;

    const source = document.createElement('div');
    source.className = 'source';
    source.style.cssText = 'font-size:0.8rem;color:var(--text-secondary);margin-top:5px;';
    source.textContent = `Source: ${event.source}`;

    div.appendChild(timestamp);
    div.appendChild(eventType);
    div.appendChild(agent);
    div.appendChild(source);

    return div;
}
