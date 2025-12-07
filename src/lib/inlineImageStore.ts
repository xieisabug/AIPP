let inlineImageCounter = 0;

interface InlineImageRecord {
    data: string;
    byteLength: number;
}

const inlineImageStore = new Map<string, InlineImageRecord>();

const DATA_PREFIX = 'base64,';

const estimateBase64Size = (dataUri: string): number => {
    const index = dataUri.indexOf(DATA_PREFIX);
    if (index === -1) {
        return Math.ceil((dataUri.length * 3) / 4);
    }
    const base64Payload = dataUri.slice(index + DATA_PREFIX.length);
    return Math.ceil((base64Payload.length * 3) / 4);
};

export const registerInlineImage = (dataUri: string): string => {
    const id = `inlineimg-${Date.now()}-${inlineImageCounter++}`;
    inlineImageStore.set(id, {
        data: dataUri,
        byteLength: estimateBase64Size(dataUri),
    });
    return id;
};

export const getInlineImage = (id?: string) => {
    if (!id) return undefined;
    return inlineImageStore.get(id);
};

export const releaseInlineImage = (id: string) => {
    inlineImageStore.delete(id);
};

export const releaseInlineImages = (ids: string[]) => {
    ids.forEach((id) => inlineImageStore.delete(id));
};

const escapeAttribute = (value: string) =>
    value.replace(/&/g, '&amp;')
        .replace(/"/g, '&quot;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');

const INLINE_IMAGE_REGEX = /!\[(.*?)\]\((data:image\/[a-zA-Z0-9+\-\.]+;base64,[^)]+)\)/g;

export const transformInlineImages = (content: string) => {
    const inlineImageIds: string[] = [];
    const transformed = content.replace(INLINE_IMAGE_REGEX, (_match, altText = '', dataUri: string) => {
        const id = registerInlineImage(dataUri);
        inlineImageIds.push(id);
        return `<inlineimage data-inline-id="${id}" data-alt="${escapeAttribute(altText)}"></inlineimage>`;
    });
    return { content: transformed, inlineImageIds };
};
