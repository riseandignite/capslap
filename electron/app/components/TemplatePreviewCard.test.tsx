import React from 'react'
import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { TemplatePreviewCard } from '../app'

const mockTemplate = {
  id: 'oneliner' as const,
  captionStyle: 'oneliner' as const,
  name: 'One Liner',
  src: 'test.mp4',
  textColor: '#ffffff',
  highlightWordColor: '#ffff00',
  outlineColor: '#000000',
  glowEffect: false,
  font: 'Montserrat Black',
  position: 'bottom' as const,
}

describe('TemplatePreviewCard', () => {
  it('renders loading spinner when isLoading is true', () => {
    render(<TemplatePreviewCard template={mockTemplate} isSelected={false} onSelect={vi.fn()} isLoading={true} />)

    expect(screen.getByTestId('preview-loading-spinner')).toBeInTheDocument()
    expect(screen.getByText('Loading preview…')).toBeInTheDocument()
  })

  it('renders preview frame image when previewFrame is provided', () => {
    const testImage = 'data:image/png;base64,mockdata'
    render(
      <TemplatePreviewCard
        template={mockTemplate}
        isSelected={false}
        onSelect={vi.fn()}
        previewFrame={testImage}
        isLoading={false}
      />
    )

    const img = screen.getByAltText('Preview')
    expect(img).toBeInTheDocument()
    expect(img).toHaveAttribute('src', testImage)
  })
})
