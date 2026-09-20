import type { Metadata } from "next";
import { CreateMarket } from "../../../components/create-market";

export const metadata: Metadata = {
  title: "Create market · Tidebook",
};

export default function CreateMarketPage() {
  return <CreateMarket />;
}
