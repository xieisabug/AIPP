const usd = new Intl.NumberFormat("en-US", {
    style: "currency", currency: "USD", minimumFractionDigits: 2, maximumFractionDigits: 12,
});
export const formatUsageDollars = (value: number) => usd.format(value);
