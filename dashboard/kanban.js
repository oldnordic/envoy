// Envoy Dashboard - Kanban View

function renderKanban() {
    // Render TODO column
    renderTaskColumn('todo', tasks.TODO);

    // Render IN PROGRESS column
    renderTaskColumn('in-progress', tasks.IN_PROGRESS);

    // Render DONE column
    renderTaskColumn('done', tasks.DONE);
}

function renderTaskColumn(columnId, taskList) {
    const container = document.getElementById(`${columnId}-tasks`);
    // Clear existing tasks
    while (container.firstChild) {
        container.removeChild(container.firstChild);
    }

    if (taskList.length === 0) {
        const emptyMsg = document.createElement('div');
        emptyMsg.textContent = 'No tasks';
        emptyMsg.style.cssText = 'color:var(--text-secondary);text-align:center;padding:20px;';
        container.appendChild(emptyMsg);
        return;
    }

    taskList.forEach(task => {
        const card = createTaskCard(task);
        container.appendChild(card);
    });
}

function createTaskCard(task) {
    const card = document.createElement('div');
    card.className = 'task-card';

    const idDiv = document.createElement('div');
    idDiv.className = 'id';
    idDiv.textContent = task.id;

    const descDiv = document.createElement('div');
    descDiv.className = 'description';
    descDiv.textContent = task.description;

    card.appendChild(idDiv);
    card.appendChild(descDiv);

    if (task.claimed_by) {
        const claimedDiv = document.createElement('div');
        claimedDiv.className = 'claimed-by';
        claimedDiv.textContent = `Claimed by: ${task.claimed_by}`;
        card.appendChild(claimedDiv);
    }

    return card;
}
