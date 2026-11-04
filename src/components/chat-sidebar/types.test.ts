import { describe, expect, it } from 'vitest';
import { getContextTypeFromToolName, isContextToolName } from './types';

describe('context tool type mapping', () => {
    it('maps preview_code to its own type instead of other/view', () => {
        expect(isContextToolName('preview_code')).toBe(true);
        expect(getContextTypeFromToolName('preview_code')).toBe('preview_code');
        expect(getContextTypeFromToolName('ui_interaction__preview_code')).toBe('preview_code');
    });

    it('keeps exact view as read_file without treating preview_* as view', () => {
        expect(isContextToolName('view')).toBe(true);
        expect(getContextTypeFromToolName('view')).toBe('read_file');
        expect(getContextTypeFromToolName('preview_file')).toBe('preview_file');
    });

    it('still recognizes search and preview_file', () => {
        expect(getContextTypeFromToolName('search_web')).toBe('search');
        expect(getContextTypeFromToolName('preview_file')).toBe('preview_file');
        expect(isContextToolName('search_web')).toBe(true);
    });
});
