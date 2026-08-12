import { describe, expect, it } from 'vitest'
import { buildRsipTreeRows, completeActiveDays, groupState } from './rsip'
import type { RsipGroup, RsipOverview, RsipTreeNode } from '../types'

function node(input: Partial<RsipTreeNode> & Pick<RsipTreeNode, 'id' | 'kind' | 'title'>): RsipTreeNode {
  return {
    state: 'active',
    deployedAt: '2026-08-12T00:00:00Z',
    groupEverEffective: false,
    version: 1,
    ...input,
  }
}

function overview(nodes: RsipTreeNode[]): RsipOverview {
  return {
    localDate: '2026-08-12',
    canDeployToday: true,
    policies: [],
    trials: [],
    groups: [],
    nodes,
    memberships: [{ groupNodeId: 'group', policyNodeId: 'member' }],
  }
}

describe('RSIP tree presentation', () => {
  it('keeps group members distinct from group dependencies', () => {
    const data = overview([
      node({ id: 'group', kind: 'group', title: 'Group' }),
      node({ id: 'member', kind: 'policy', title: 'Member' }),
      node({ id: 'dependent', kind: 'policy', title: 'Dependent', parentNodeId: 'group' }),
    ])
    expect(buildRsipTreeRows(data).map((row) => [row.node.id, row.relation, row.depth])).toEqual([
      ['group', 'root', 0],
      ['member', 'member', 1],
      ['dependent', 'dependency', 1],
    ])
  })

  it('distinguishes forming, effective and degraded groups', () => {
    const data = overview([
      node({ id: 'group', kind: 'group', title: 'Group' }),
      node({ id: 'member', kind: 'policy', title: 'Member' }),
    ])
    const group: RsipGroup = {
      id: 'definition', name: 'Group', targetMemberCount: 3, minimumActiveMembers: 2,
      version: 1, createdAt: '', updatedAt: '',
    }
    expect(groupState(data, data.nodes[0]!, group)).toBe('forming')
    group.minimumActiveMembers = 1
    expect(groupState(data, data.nodes[0]!, group)).toBe('effective')
    group.minimumActiveMembers = 2
    data.nodes[0]!.groupEverEffective = true
    expect(groupState(data, data.nodes[0]!, group)).toBe('degraded')
  })

  it('counts only complete active days', () => {
    expect(completeActiveDays(86_399)).toBe(0)
    expect(completeActiveDays(172_801)).toBe(2)
  })
})
