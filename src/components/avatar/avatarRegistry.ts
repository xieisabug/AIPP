import { useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
    accessoryDefinitions,
    avatarDefinitions,
    isPluginAssetId,
    type AccessoryAnchor,
    type AccessoryEntry,
    type AccessoryId,
    type AvatarAction,
    type AvatarAnchor,
    type AvatarDefinition,
    type AvatarShape,
    type BuiltinAccessoryId,
    type BuiltinAvatarShape,
    type PluginAccessoryDefinition,
    type PluginAssetId,
} from "./avatarDefinition";
import type { VectorLayer } from "./DeclarativeArtwork";

const STORAGE_KEY = "aipp-plugin-avatar-registry-v1";

export interface PluginAvatarAsset {
    id: string;
    localId: string;
    pluginCode: string;
    pluginName: string;
    label: string;
    description?: string;
    vectorParts: VectorLayer[];
    artworkScale?: number;
    eyeColor?: string;
    eyeScale?: number;
    faceColorSlot?: "primary" | "secondary" | "white";
    defaultColors?: { primary?: string; secondary?: string };
    face: AvatarAnchor;
    crown: AvatarAnchor;
    headphones: AvatarAnchor & { scaleY?: number };
    clip: AvatarAnchor;
    propDock: AvatarAnchor;
    colorLabels: { primary?: string; secondary: string };
    capabilities: ("eyes" | "headwear")[];
}

export interface PluginAccessoryAsset {
    id: string;
    localId: string;
    pluginCode: string;
    pluginName: string;
    label: string;
    description?: string;
    slot: "eyes" | "head";
    anchor: AccessoryAnchor;
    followGaze: boolean;
    action?: AvatarAction | null;
    previewViewBox: string;
    layers: VectorLayer[];
}

export interface PluginAvatarAssetBundle {
    avatars: PluginAvatarAsset[];
    accessories: PluginAccessoryAsset[];
}

export interface AvatarRegistrySnapshot {
    avatars: AvatarDefinition[];
    pluginAvatars: AvatarDefinition[];
    accessories: AccessoryEntry[];
    pluginAccessories: PluginAccessoryDefinition[];
    avatarMap: ReadonlyMap<string, AvatarDefinition>;
    accessoryMap: ReadonlyMap<string, AccessoryEntry>;
}

const builtinAvatars: AvatarDefinition[] = Object.values(avatarDefinitions);
const builtinAccessories: AccessoryEntry[] = (Object.keys(accessoryDefinitions) as BuiltinAccessoryId[]).map(id => {
    const def = accessoryDefinitions[id];
    return {
        id,
        slot: def.slot,
        label: def.label,
        capability: def.capability,
        action: def.action,
    };
});

export function toAvatarDefinition(asset: PluginAvatarAsset): AvatarDefinition | null {
    if (!isPluginAssetId(asset.id)) return null;
    return {
        id: asset.id,
        label: asset.label,
        description: asset.description,
        parts: [],
        vectorParts: asset.vectorParts,
        artworkScale: asset.artworkScale,
        eyeColor: asset.eyeColor,
        eyeScale: asset.eyeScale,
        faceColorSlot: asset.faceColorSlot,
        defaultColors: asset.defaultColors,
        face: asset.face,
        crown: asset.crown,
        headphones: asset.headphones,
        clip: asset.clip,
        propDock: asset.propDock,
        colorLabels: asset.colorLabels,
        capabilities: asset.capabilities,
        pluginCode: asset.pluginCode,
        pluginName: asset.pluginName,
    };
}

export function toPluginAccessoryDefinition(asset: PluginAccessoryAsset): PluginAccessoryDefinition | null {
    if (!isPluginAssetId(asset.id)) return null;
    return {
        id: asset.id,
        slot: asset.slot,
        label: asset.label,
        description: asset.description,
        capability: asset.slot === "eyes" ? "eyes" : "headwear",
        anchor: asset.anchor,
        followGaze: asset.followGaze,
        action: asset.action ?? null,
        previewViewBox: asset.previewViewBox,
        layers: asset.layers,
        pluginCode: asset.pluginCode,
        pluginName: asset.pluginName,
    };
}

function buildSnapshot(bundle: PluginAvatarAssetBundle): AvatarRegistrySnapshot {
    const pluginAvatars: AvatarDefinition[] = [];
    const avatarMap = new Map<string, AvatarDefinition>();
    for (const builtin of builtinAvatars) {
        avatarMap.set(builtin.id, builtin);
    }
    for (const asset of bundle.avatars) {
        const def = toAvatarDefinition(asset);
        if (!def || avatarMap.has(def.id)) continue;
        avatarMap.set(def.id, def);
        pluginAvatars.push(def);
    }

    const pluginAccessories: PluginAccessoryDefinition[] = [];
    const accessoryMap = new Map<string, AccessoryEntry>();
    for (const builtin of builtinAccessories) {
        accessoryMap.set(builtin.id, builtin);
    }
    for (const asset of bundle.accessories) {
        const def = toPluginAccessoryDefinition(asset);
        if (!def || accessoryMap.has(def.id)) continue;
        const entry: AccessoryEntry = {
            id: def.id,
            slot: def.slot,
            label: def.label,
            description: def.description,
            capability: def.capability,
            action: def.action,
            plugin: def,
        };
        accessoryMap.set(def.id, entry);
        pluginAccessories.push(def);
    }

    return {
        avatars: [...builtinAvatars, ...pluginAvatars],
        pluginAvatars,
        accessories: [...builtinAccessories, ...pluginAccessories.map(p => accessoryMap.get(p.id)!)],
        pluginAccessories,
        avatarMap,
        accessoryMap,
    };
}

function readCachedBundle(): PluginAvatarAssetBundle {
    if (typeof window === "undefined") return { avatars: [], accessories: [] };
    try {
        const raw = window.localStorage.getItem(STORAGE_KEY);
        if (!raw) return { avatars: [], accessories: [] };
        const parsed = JSON.parse(raw);
        if (!parsed || typeof parsed !== "object") return { avatars: [], accessories: [] };
        return {
            avatars: Array.isArray(parsed.avatars) ? parsed.avatars : [],
            accessories: Array.isArray(parsed.accessories) ? parsed.accessories : [],
        };
    } catch {
        return { avatars: [], accessories: [] };
    }
}

function persistCachedBundle(bundle: PluginAvatarAssetBundle): void {
    if (typeof window === "undefined") return;
    try {
        window.localStorage.setItem(STORAGE_KEY, JSON.stringify(bundle));
    } catch {
        // Ignore storage quota or sandbox failures.
    }
}

class AvatarRegistry {
    private snapshot: AvatarRegistrySnapshot = buildSnapshot(readCachedBundle());
    private listeners = new Set<() => void>();
    private syncPromise: Promise<void> | null = null;

    constructor() {
        if (typeof window !== "undefined") {
            window.addEventListener("storage", event => {
                if (event.key !== STORAGE_KEY) return;
                this.snapshot = buildSnapshot(readCachedBundle());
                this.emitChange();
            });
        }
    }

    subscribe(listener: () => void): () => void {
        this.listeners.add(listener);
        return () => {
            this.listeners.delete(listener);
        };
    }

    getSnapshot(): AvatarRegistrySnapshot {
        return this.snapshot;
    }

    getAvatarDefinition(shape: string): AvatarDefinition | undefined {
        return this.snapshot.avatarMap.get(shape);
    }

    resolveAvatarDefinition(shape: AvatarShape): { definition: AvatarDefinition; fallback: boolean } {
        const matched = this.snapshot.avatarMap.get(shape);
        if (matched) return { definition: matched, fallback: false };
        return { definition: avatarDefinitions.pebble, fallback: true };
    }

    getAccessoryEntry(id: string | null | undefined): AccessoryEntry | undefined {
        if (!id) return undefined;
        return this.snapshot.accessoryMap.get(id);
    }

    setPluginAssets(bundle: PluginAvatarAssetBundle, persist = true): void {
        const normalized: PluginAvatarAssetBundle = {
            avatars: Array.isArray(bundle.avatars) ? bundle.avatars : [],
            accessories: Array.isArray(bundle.accessories) ? bundle.accessories : [],
        };
        this.snapshot = buildSnapshot(normalized);
        if (persist) {
            persistCachedBundle(normalized);
        }
        this.emitChange();
    }

    clearPluginAssets(persist = true): void {
        this.setPluginAssets({ avatars: [], accessories: [] }, persist);
    }

    async syncFromBackend(): Promise<void> {
        if (this.syncPromise) return this.syncPromise;
        this.syncPromise = (async () => {
            try {
                const bundle = await invoke<PluginAvatarAssetBundle>("list_plugin_avatar_assets");
                if (bundle && typeof bundle === "object") {
                    this.setPluginAssets(bundle, true);
                }
            } catch (error) {
                console.warn("[AvatarRegistry] Failed to sync plugin avatar assets:", error);
            }
        })().finally(() => {
            this.syncPromise = null;
        });
        return this.syncPromise;
    }

    private emitChange(): void {
        this.listeners.forEach(listener => listener());
    }
}

export const avatarRegistry = new AvatarRegistry();

export function useAvatarRegistrySnapshot(): AvatarRegistrySnapshot {
    return useSyncExternalStore(
        listener => avatarRegistry.subscribe(listener),
        () => avatarRegistry.getSnapshot(),
        () => avatarRegistry.getSnapshot(),
    );
}

export function isBuiltinAvatarShape(value: string): value is BuiltinAvatarShape {
    return Object.hasOwn(avatarDefinitions, value);
}

export function isBuiltinAccessoryId(value: string): value is BuiltinAccessoryId {
    return Object.hasOwn(accessoryDefinitions, value);
}

export function extractPluginCodeFromAssetId(id: string): string | null {
    if (!isPluginAssetId(id)) return null;
    const colon = id.indexOf(":");
    return colon > 0 ? id.slice(0, colon) : null;
}

export type { AccessoryId, AvatarShape, PluginAssetId };
