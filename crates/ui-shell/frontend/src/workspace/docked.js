// A saved page from before every panel was docked may hold groups floating
// over it, or popped out of it. Their panels join the dock's first group as
// tabs, so the page loads with nothing lost and nothing floating.

const leafOf = (node) => {
  if (!node) return null;
  if (node.type === "leaf") return node;
  for (const child of Array.isArray(node.data) ? node.data : []) {
    const leaf = leafOf(child);
    if (leaf) return leaf;
  }
  return null;
};

/**
 * Saved dockview JSON with its floating and popped-out groups' panels moved
 * into the grid. JSON without any comes back as it is.
 */
export function dockedLayout(json) {
  const groups = [...(json?.floatingGroups ?? []), ...(json?.popoutGroups ?? [])];
  if (groups.length === 0) return json;
  const lifted = groups
    .map((group) => group?.data)
    .filter((data) => Array.isArray(data?.views) && data.views.length > 0);
  if (lifted.length === 0) return { ...json, floatingGroups: [], popoutGroups: [] };
  const docked = structuredClone(json);
  docked.grid ??= { orientation: "HORIZONTAL", width: 0, height: 0 };
  let leaf = leafOf(docked.grid.root);
  if (!leaf) {
    // A branch's children run across its orientation: a horizontal grid's
    // root lays them out side by side, so each takes a share of the width.
    const across = docked.grid.orientation === "VERTICAL" ? docked.grid.height : docked.grid.width;
    const along = docked.grid.orientation === "VERTICAL" ? docked.grid.width : docked.grid.height;
    leaf = { type: "leaf", data: { ...lifted[0], views: [] }, size: across || 0 };
    docked.grid.root = { type: "branch", data: [leaf], size: docked.grid.root?.size ?? (along || 0) };
  }
  const views = [...(leaf.data.views ?? [])];
  for (const data of lifted) {
    for (const view of data.views) if (!views.includes(view)) views.push(view);
  }
  leaf.data = { ...leaf.data, views, activeView: leaf.data.activeView ?? views[0] };
  const groupIds = new Set(lifted.map((data) => data.id));
  return {
    ...docked,
    floatingGroups: [],
    popoutGroups: [],
    activeGroup: groupIds.has(docked.activeGroup) ? leaf.data.id : docked.activeGroup,
  };
}
