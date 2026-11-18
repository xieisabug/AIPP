import { describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { shareConfig, reviewRefresh, checkFailure } from './connectionManagement';
import type { AppConnection } from '@/data/ConnectedApp';
import type { MCPServer } from '@/data/MCP';


describe('connection management', () => {
    it('should exclude identity and secrets when sharing catalog, HTTP and local connections', () => {
        const server = {name:'PRIVATE', description:'PRIVATE', transport_type:'http', command:'program PRIVATE',
            url:'https://PRIVATE.example/path/PRIVATE?key=PRIVATE', headers:'PRIVATE', environment_variables:'PRIVATE'} as MCPServer;
        const connection = {app_id:'notes',label:'PRIVATE',config:{account:'PRIVATE'}} as unknown as AppConnection;
        expect(shareConfig(server,connection)).toEqual({format:'aipp.connection.v1',app:'notes'});
        for(const transport_type of ['http','stdio']) expect(JSON.stringify(shareConfig({...server,transport_type}))).not.toContain('PRIVATE');
    });
    it('should keep the old tools when the user declines a refresh', async () => {
        vi.mocked(invoke).mockResolvedValue({token:'receipt',added:['write'],changed:[],removed:[]});
        const confirm = vi.fn().mockResolvedValue(false);
        expect(await reviewRefresh(14, confirm)).toBe(false);
        expect(confirm).toHaveBeenCalledWith(expect.objectContaining({added:['write']}));
        expect(invoke).not.toHaveBeenCalledWith('apply_connection_refresh',expect.anything());
    });
    it('should distinguish rejected credentials, insufficient permissions, unsupported services and network failures', () => {
        expect(new Set(['HTTP 401','HTTP 403','unsupported','connection timed out'].map(checkFailure)).size).toBe(4);
    });
});
