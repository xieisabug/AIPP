import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, it, expect, vi } from 'vitest';
import JSONImportDialog from './JSONImportDialog';
vi.mock('@tauri-apps/plugin-clipboard-manager', () => ({readText: vi.fn(async () => '')}));

describe('JSON connection import', () => {
    it('should preview every local command and ignore incoming grants before saving', async () => {
        const onImport = vi.fn();
        render(<JSONImportDialog isOpen onClose={vi.fn()} onImport={onImport}/>);
        fireEvent.change(screen.getByLabelText('JSON配置'), {target:{value: JSON.stringify({
            mcpServers:{first:{command:'placeholder',args:['--sample'],is_enabled:true},second:{url:'https://example.com/mcp'}}
        })}});
        fireEvent.click(screen.getByRole('button',{name:'预览配置'}));
        expect(await screen.findByDisplayValue('placeholder --sample')).toBeInTheDocument();
        expect(onImport).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole('button',{name:'保存为停用连接'}));
        await waitFor(() => expect(onImport).toHaveBeenCalledWith([
            expect.objectContaining({name:'first', is_enabled:false}), expect.objectContaining({name:'second', is_enabled:false}),
        ]));
    });
    it('should open the public catalog entry without importing a connection or trusting extra fields', async () => {
        const onImport=vi.fn(), onCatalog=vi.fn();
        render(<JSONImportDialog isOpen onClose={vi.fn()} onImport={onImport} onCatalog={onCatalog}/>);
        fireEvent.change(screen.getByLabelText('JSON配置'), {target:{value: JSON.stringify({
            format:'aipp.connection.v1',app:'notes',command:'unexpected',url:'https://example.com/private'
        })}});
        fireEvent.click(screen.getByRole('button',{name:'预览配置'}));
        fireEvent.click(await screen.findByRole('button',{name:'查看目录详情'}));
        await waitFor(() => expect(onCatalog).toHaveBeenCalledWith('notes'));
        expect(onImport).not.toHaveBeenCalled();
    });
});
