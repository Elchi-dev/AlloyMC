import React from "react";
import ReactDOM from "react-dom/client";
import { PersistQueryClientProvider } from "@tanstack/react-query-persist-client";
import { createSyncStoragePersister } from "@tanstack/query-sync-storage-persister";
import App from "./App";
import { queryClient } from "./lib/queries";
import "./styles.css";

const persister = createSyncStoragePersister({ storage: window.localStorage, key: "alloy-cache", throttleTime: 1000 });

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <PersistQueryClientProvider
      client={queryClient}
      persistOptions={{
        persister,
        maxAge: 7 * 24 * 3600 * 1000,
        // Only local, cheap-to-be-stale data: the first frame renders it while fresh data loads.
        dehydrateOptions: {
          shouldDehydrateQuery: (q) => ["instances", "accounts", "settings", "versions", "content"].includes(String(q.queryKey[0])),
        },
      }}
    >
      <App />
    </PersistQueryClientProvider>
  </React.StrictMode>,
);
