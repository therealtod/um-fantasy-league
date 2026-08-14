import { describe, expect, it, vi } from 'vitest'
import { debounce } from './debounce'

describe('debounce', () => {
  it('collapses a burst of calls into one, using the last call\'s args', () => {
    vi.useFakeTimers()
    const fn = vi.fn()
    const debounced = debounce(fn, 300)

    debounced('m')
    debounced('me')
    debounced('med')

    vi.advanceTimersByTime(299)
    expect(fn).not.toHaveBeenCalled()

    vi.advanceTimersByTime(1)
    expect(fn).toHaveBeenCalledTimes(1)
    expect(fn).toHaveBeenCalledWith('med')

    vi.useRealTimers()
  })

  it('fires again after a fresh wait once the previous call has settled', () => {
    vi.useFakeTimers()
    const fn = vi.fn()
    const debounced = debounce(fn, 300)

    debounced('a')
    vi.advanceTimersByTime(300)
    expect(fn).toHaveBeenCalledTimes(1)

    debounced('b')
    vi.advanceTimersByTime(300)
    expect(fn).toHaveBeenCalledTimes(2)
    expect(fn).toHaveBeenLastCalledWith('b')

    vi.useRealTimers()
  })
})
