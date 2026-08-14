import { mount } from '@vue/test-utils'
import { describe, expect, it } from 'vitest'
import ErrorBanner from './ErrorBanner.vue'

describe('ErrorBanner', () => {
  it('renders nothing when there is no message', () => {
    const wrapper = mount(ErrorBanner, { props: { message: null } })
    expect(wrapper.find('p, ul').exists()).toBe(false)
  })

  it('renders a plain message when there are no structured violations', () => {
    const wrapper = mount(ErrorBanner, { props: { message: 'Failed to save tournament' } })
    expect(wrapper.find('ul').exists()).toBe(false)
    expect(wrapper.text()).toBe('Failed to save tournament')
  })

  it('renders one list item per violation instead of a single joined sentence', () => {
    // This is what a *RuleException's `describeError` message looks like — semicolon-joined —
    // vs. the structured `violations` array `violationMessages` recovers from the same ApiError.
    const wrapper = mount(ErrorBanner, {
      props: {
        message: 'Exactly one winner is required; The loser must have 0 or less health',
        violations: ['Exactly one winner is required', 'The loser must have 0 or less health'],
      },
    })

    const items = wrapper.findAll('li')
    expect(items.map((li) => li.text())).toEqual([
      'Exactly one winner is required',
      'The loser must have 0 or less health',
    ])
    // The joined sentence never renders alongside the list.
    expect(wrapper.find('p').exists()).toBe(false)
  })

  it('falls back to the plain message for a single-item violations array', () => {
    const wrapper = mount(ErrorBanner, {
      props: { message: 'Tournament name is required', violations: ['Tournament name is required'] },
    })
    expect(wrapper.find('ul').exists()).toBe(false)
    expect(wrapper.text()).toBe('Tournament name is required')
  })
})
