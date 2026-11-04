/**
 * Assistant form dirty check that compares what the user can actually change.
 *
 * react-hook-form's `formState.isDirty` deep-compares the whole value object with the
 * reset() defaults, and its deepEqual fails as soon as the key counts differ. Every
 * Controller whose name was not in the reset object registers itself with an
 * `undefined` value, so the form turned "dirty" without any edit and the leave prompt
 * appeared out of nowhere. Here missing / undefined / null / "" are equivalent, and
 * scalars compare by string form ("0.7" vs 0.7, "true" vs true) because model configs
 * round-trip through string storage.
 */
const normalizeFormValue = (value: unknown): string => {
    if (value === undefined || value === null) return "";
    if (typeof value === "object") return JSON.stringify(value);
    return String(value);
};

export function isAssistantFormDirty(
    values: Record<string, unknown>,
    baseline: Record<string, unknown>,
): boolean {
    const keys = new Set([...Object.keys(values), ...Object.keys(baseline)]);
    for (const key of keys) {
        if (normalizeFormValue(values[key]) !== normalizeFormValue(baseline[key])) return true;
    }
    return false;
}
