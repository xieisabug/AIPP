import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { invoke } from '@tauri-apps/api/core';
import AppRelatedSkills from './AppRelatedSkills';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('../SkillInstallGuideDialog', () => ({ default: ({ officialSkillIds }: { officialSkillIds: string[] }) => <div>安装向导 {officialSkillIds.join(',')}</div> }));

describe('AppRelatedSkills', () => {
    beforeEach(() => vi.clearAllMocks());
    it('should show only declared skills and require opening the installer when the catalog resolves', async () => {
        vi.mocked(invoke).mockResolvedValue([{ id: 'notes', name: '笔记技能', description: '读取笔记' }, { id: 'other', name: '无关技能' }]);
        const user = userEvent.setup();
        render(<AppRelatedSkills ids={['notes', 'missing']} />);
        expect(await screen.findByText('笔记技能')).toBeInTheDocument();
        expect(screen.queryByText('无关技能')).not.toBeInTheDocument();
        expect(screen.getByText('missing')).toBeInTheDocument();
        expect(screen.queryByText(/安装向导/)).not.toBeInTheDocument();
        expect(invoke).toHaveBeenCalledTimes(1);
        await user.click(screen.getByRole('button', { name: '查看并安装' }));
        expect(screen.getByText('安装向导 notes')).toBeInTheDocument();
    });
    it('should keep an unavailable skill explicit when the catalog request fails', async () => {
        vi.mocked(invoke).mockRejectedValue(new Error('目录离线'));
        render(<AppRelatedSkills ids={['notes']} />);
        expect(await screen.findByRole('alert')).toHaveTextContent('目录离线');
        expect(screen.queryByRole('button', { name: '查看并安装' })).not.toBeInTheDocument();
    });
});
