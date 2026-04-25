import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, afterEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

import ImageAttachments from './ImageAttachments';

describe('ImageAttachments', () => {
    afterEach(() => {
        vi.clearAllMocks();
    });

    it('opens image attachments with the system image command when clicked', async () => {
        vi.mocked(invoke).mockResolvedValue(undefined);

        render(
            <ImageAttachments
                conversationId={42}
                messageId={101}
                attachments={[
                    {
                        id: 7,
                        attachment_type: 'Image',
                        attachment_url: 'generated-image-1.png',
                        attachment_content: 'data:image/png;base64,Zm9v',
                    },
                ]}
            />
        );

        fireEvent.click(screen.getByRole('button', { name: '点击使用系统默认程序打开图片' }));

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith('open_image', {
                imageData: 'data:image/png;base64,Zm9v',
                conversationId: '42',
                messageId: '7',
            });
        });
    });

    it('falls back to attachment_url when image content is unavailable', async () => {
        vi.mocked(invoke).mockResolvedValue(undefined);

        render(
            <ImageAttachments
                conversationId={42}
                messageId={101}
                attachments={[
                    {
                        attachment_type: 'Image',
                        attachment_url: 'https://example.com/image.png',
                    },
                ]}
            />
        );

        fireEvent.click(screen.getByRole('button', { name: '点击使用系统默认程序打开图片' }));

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith('open_image', {
                imageData: 'https://example.com/image.png',
                conversationId: '42',
                messageId: '101',
            });
        });
    });

    it('does not render when there are no image attachments', () => {
        const { container } = render(
            <ImageAttachments
                attachments={[
                    {
                        attachment_type: 'Text',
                        attachment_url: 'note.txt',
                        attachment_content: 'hello',
                    },
                ]}
            />
        );

        expect(container).toBeEmptyDOMElement();
    });

    it('renders images with contain sizing to preserve aspect ratio', () => {
        render(
            <ImageAttachments
                attachments={[
                    {
                        attachment_type: 'Image',
                        attachment_url: 'generated-image-1.png',
                        attachment_content: 'data:image/png;base64,Zm9v',
                    },
                ]}
            />
        );

        expect(screen.getByRole('img')).toHaveClass('object-contain');
        expect(screen.getByRole('img')).toHaveClass('h-auto');
    });
});
