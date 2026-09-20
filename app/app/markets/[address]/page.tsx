import { MarketDetail } from "../../../components/market-detail";

export default async function MarketDetailPage({
  params,
}: {
  params: Promise<{ address: string }>;
}) {
  const { address } = await params;
  return <MarketDetail address={address} />;
}
