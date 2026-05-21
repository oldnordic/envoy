// Envoy Dashboard - Graph View (D3.js Force-Directed Graph)

function renderGraph() {
    const container = document.getElementById('graph');
    
    // Clear previous content using DOM methods
    while (container.firstChild) {
        container.removeChild(container.firstChild);
    }

    if (nodes.length === 0) {
        const emptyMsg = document.createElement('div');
        emptyMsg.textContent = 'No nodes to display';
        emptyMsg.style.cssText = 'display:flex;align-items:center;justify-content:center;height:100%;color:var(--text-secondary);';
        container.appendChild(emptyMsg);
        return;
    }

    const width = container.clientWidth;
    const height = container.clientHeight;

    // Create SVG with a wrapper group for zoom/pan
    const svg = d3.select('#graph')
        .append('svg')
        .attr('width', width)
        .attr('height', height);

    const g = svg.append('g');

    // Initialize nodes at center to prevent off-screen positioning
    nodes.forEach(n => {
        n.x = width / 2;
        n.y = height / 2;
    });

    // Create force simulation
    const simulation = d3.forceSimulation(nodes)
        .force('link', d3.forceLink(edges)
            .id(d => d.id)
            .distance(30))
        .force('center', d3.forceCenter(width / 2, height / 2))
        .force('collision', d3.forceCollide()
            .radius(8));

    // Create links (in wrapper group)
    const link = g.append('g')
        .selectAll('line')
        .data(edges)
        .join('line')
        .attr('class', 'link')
        .attr('stroke', '#4cc9f0')
        .attr('stroke-width', 1)
        .attr('stroke-opacity', 0.6);

    // Create nodes (in wrapper group)
    const node = g.append('g')
        .selectAll('.node')
        .data(nodes)
        .join('g')
        .attr('class', d => `node ${d.kind.toLowerCase()}`)
        .call(d3.drag()
            .on('start', dragstarted)
            .on('drag', dragged)
            .on('end', dragended));

    // Add circles to nodes
    node.append('circle')
        .attr('r', d => {
            if (d.kind === 'Agent') return 8;
            if (d.kind === 'Discovery') return 5;
            return 4;
        });

    // Add labels
    node.append('text')
        .attr('x', 12)
        .attr('y', 3)
        .text(d => d.label);

    // Update positions on tick
    simulation.on('tick', () => {
        link
            .attr('x1', d => d.source.x)
            .attr('y1', d => d.source.y)
            .attr('x2', d => d.target.x)
            .attr('y2', d => d.target.y);

        node
            .attr('transform', d => `translate(${d.x},${d.y})`);
    });

    // Drag functions
    function dragstarted(event, d) {
        if (!event.active) simulation.alphaTarget(0.3).restart();
        d.fx = d.x;
        d.fy = d.y;
    }

    function dragged(event, d) {
        d.fx = event.x;
        d.fy = event.y;
    }

    function dragended(event, d) {
        if (!event.active) simulation.alphaTarget(0);
        d.fx = null;
        d.fy = null;
    }

    // Zoom behavior
    const zoom = d3.zoom()
        .scaleExtent([0.1, 4])
        .on('zoom', (event) => {
            g.attr('transform', event.transform);
        });

    svg.call(zoom);
}
