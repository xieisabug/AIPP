import { describe, it, expect } from 'vitest';
import type { AppConnection } from '@/data/ConnectedApp';
import { aggregateStatus, displayStatus, toolSummary } from './appStatus';

const connection = (status: AppConnection['status'], error: string | null = null) => ({ status, error } as AppConnection);
const enabled = { is_enabled: true, is_builtin: false };
const disabled = { is_enabled: false, is_builtin: false };

describe('displayStatus', () => {
    it('should surface authorization problems even when the server is disabled', () => {
        // Backend disables the server on auth failure; "已停用" would hide that re-login is required.
        expect(displayStatus(disabled, connection('auth_required'))).toMatchObject({ label: '需要重新登录', attention: true });
        expect(displayStatus(disabled, connection('connecting'))).toMatchObject({ label: '等待授权', attention: true });
    });
    it('should report a disabled server as stopped when its last verification succeeded', () => {
        expect(displayStatus(disabled, connection('available'))).toMatchObject({ label: '已停用', attention: false });
    });
    it('should only ask for attention after a disconnect when revocation failed', () => {
        expect(displayStatus(disabled, connection('disconnected'))).toMatchObject({ label: '已断开', attention: false });
        expect(displayStatus(disabled, connection('disconnected', '服务商撤销未完成'))).toMatchObject({ label: '断开未完成', attention: true });
    });
    it('should mark a device-unsupported server regardless of connection state', () => {
        expect(displayStatus(enabled, connection('error'), false)).toMatchObject({ label: '此设备不可执行', attention: false });
    });
    it('should treat enabled plain MCP servers as enabled rather than verified', () => {
        expect(displayStatus(enabled)).toMatchObject({ label: '已启用' });
        expect(displayStatus(enabled, connection('unchecked'))).toMatchObject({ label: '待验证', tone: 'idle' });
    });
});

describe('displayStatus on a device without the credential', () => {
    const synced = (status: AppConnection['status'], credential_sync: AppConnection['credential_sync']) =>
        ({ status, error: null, credential_on_device: false, credential_sync } as AppConnection);
    it('should not ask this device to re-login when it never had the credential', () => {
        // auth_required synced from another device must not offer a re-login that would replace its credential.
        expect(displayStatus(enabled, synced('auth_required', 'not_configured'))).toMatchObject({ label: '此设备无凭据', attention: false });
        expect(displayStatus(enabled, synced('available', 'ready'))).toMatchObject({ label: '此设备无凭据', attention: false });
    });
    it('should ask for attention when entering the sync passphrase would fix it', () => {
        expect(displayStatus(enabled, synced('unchecked', 'needs_passphrase'))).toMatchObject({ label: '凭据待解锁', tone: 'warn', attention: true });
        expect(displayStatus(enabled, synced('unchecked', 'mismatch'))).toMatchObject({ attention: true });
    });
    it('should keep a deliberate disconnect visible', () => {
        expect(displayStatus(disabled, synced('disconnected', 'not_configured'))).toMatchObject({ label: '已断开' });
    });
});

describe('toolSummary', () => {
    it('should show unknown instead of zero when tools were never discovered', () => {
        expect(toolSummary([], enabled, connection('unchecked')).label).toBe('工具未检测');
        expect(toolSummary([], enabled, connection('available')).label).toBe('0 / 0 个工具');
        expect(toolSummary([], { is_builtin: true }).label).toBe('0 / 0 个工具');
    });
});

describe('aggregateStatus', () => {
    it('should keep the worst tone and count connections that need attention', () => {
        const result = aggregateStatus([displayStatus(enabled, connection('available')), displayStatus(disabled, connection('auth_required'))]);
        expect(result).toMatchObject({ label: '2 个连接 · 1 个需处理', tone: 'warn', attention: true });
        expect(aggregateStatus([])).toBeUndefined();
    });
});
