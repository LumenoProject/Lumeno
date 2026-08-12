import type { RsipGroup, RsipOverview, RsipTreeNode } from '../types'

export interface RsipTreeRow {
  node: RsipTreeNode
  depth: number
  relation: 'root' | 'dependency' | 'member'
}

export type RsipGroupState = 'forming' | 'effective' | 'degraded'

export function activeMemberCount(overview: RsipOverview, groupNodeId: string): number {
  const active = new Set(overview.nodes.map((node) => node.id))
  return overview.memberships.filter((item) =>
    item.groupNodeId === groupNodeId && active.has(item.policyNodeId)).length
}

export function groupState(
  overview: RsipOverview,
  node: RsipTreeNode,
  group: RsipGroup,
): RsipGroupState {
  if (activeMemberCount(overview, node.id) >= group.minimumActiveMembers) return 'effective'
  return node.groupEverEffective ? 'degraded' : 'forming'
}

export function buildRsipTreeRows(overview: RsipOverview): RsipTreeRow[] {
  const nodes = new Map(overview.nodes.map((node) => [node.id, node]))
  const membersByGroup = new Map<string, RsipTreeNode[]>()
  const memberIds = new Set<string>()
  for (const membership of overview.memberships) {
    const member = nodes.get(membership.policyNodeId)
    if (!member) continue
    memberIds.add(member.id)
    const members = membersByGroup.get(membership.groupNodeId) ?? []
    members.push(member)
    membersByGroup.set(membership.groupNodeId, members)
  }

  const childrenByParent = new Map<string, RsipTreeNode[]>()
  for (const node of overview.nodes) {
    if (!node.parentNodeId || !nodes.has(node.parentNodeId)) continue
    const children = childrenByParent.get(node.parentNodeId) ?? []
    children.push(node)
    childrenByParent.set(node.parentNodeId, children)
  }

  const roots = overview.nodes.filter((node) =>
    !memberIds.has(node.id) && (!node.parentNodeId || !nodes.has(node.parentNodeId)))
  const rows: RsipTreeRow[] = []
  const seen = new Set<string>()
  const stack: RsipTreeRow[] = roots.toReversed().map((node) => ({ node, depth: 0, relation: 'root' }))
  while (stack.length) {
    const row = stack.pop()
    if (!row || seen.has(row.node.id)) continue
    seen.add(row.node.id)
    rows.push({ ...row, depth: Math.min(row.depth, 8) })
    const dependencies = childrenByParent.get(row.node.id) ?? []
    const members = row.node.kind === 'group' ? membersByGroup.get(row.node.id) ?? [] : []
    const next = [
      ...members.map((node) => ({ node, depth: row.depth + 1, relation: 'member' as const })),
      ...dependencies.map((node) => ({ node, depth: row.depth + 1, relation: 'dependency' as const })),
    ]
    stack.push(...next.toReversed())
  }

  for (const node of overview.nodes) {
    if (!seen.has(node.id)) rows.push({ node, depth: 0, relation: 'root' })
  }
  return rows
}

export function completeActiveDays(totalActiveSeconds: number): number {
  return Math.max(0, Math.floor(totalActiveSeconds / 86_400))
}
