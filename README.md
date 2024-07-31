# Solforge Home Case Study

This is the solution of Jonathan Thaler for the Solforge Home Case Study, based on the requirements as specified in [Case Study: Building a Solana Blockchain Data Aggregator](https://solforge.notion.site/Home-Case-Study-9384e162b64143df9a729ee482f10fda). The exact requirements are copied below as preceeding context for the specifications secion.s

**Context:**
You are assigned to develop a mini Data aggregator software that collects and processes data from the Solana blockchain. The goal is to create a system capable of retrieving transaction and account data for the ongoing epoch.

**Our Goal:**

Keep in mind that we are not expecting you to create a production-ready software (docker, ci, prometheus metrics…) but to understand your level in Rust and the basics of system design. We are still expecting the code to be somehow clean as it will reflect you Rust level (eg. handling errors, decent project composition, etc).

**Resources:**

<aside>
💡 To get a node on devnet, you can go to https://www.helius.dev/ and create a free account. However, you can choose the provider of your choice.
</aside>

**Requirements:**

1. **Data Retrieval:** Develop a Rust application capable of retrieving transaction and account data from the Solana blockchain on devnet or testnet. Utilise Solana's API or SDK to interact with the blockchain and fetch relevant data.
2. **Data Processing:** Implement mechanisms to process the retrieved data efficiently. This includes parsing transaction records, extracting relevant information such as sender, receiver, amount, timestamp, etc., and organising data into a structured format for further analysis and queries.
3. **Data History:** Configure the data aggregator to start aggregating data from the current epoch and onwards. Exclude historical data to focus on recent transactions and account changes. Ensure the data aggregator provides real-time updates by continuously monitoring the blockchain for new transactions and account changes.
4. **Data Storage (optional):** Choose a storage solution to store the collected data securely. Consider using a suitable database or data storage mechanism that offers scalability, reliability, and fast query capabilities. If you are running out of time, a in-memory structure is enough!
5. **API Integration:** Create a RESTful API layer to expose the aggregated data to external systems and applications. The API should support various queries to retrieve transaction history, account details, and other relevant information.

<aside>
💡 route (transactions)

1. transactions/?id=4CqYTMNtGpWjk67Ntq9QtDHZNaDeqYwhbh6cMVx7Qx6Y4b43kgsHP8t4TJbdrWf5kD4xuWNXhFLZfo4H6GBmxXzG
2. transactions/?day=23/05/2023
</aside>

**Deliverables:**

1. **Data Aggregator Application:** Develop a Rust-based application that fulfils the requirements outlined above. The application should be well-structured, modular, and decently documented to facilitate the review and highlight some enhancements.
2. **Documentation:** Provide in the README a few notes covering the architecture, design decisions, API endpoints, usage instructions, and any other relevant information necessary for deploying and using the data aggregator tool.
3. **Testing:** Write tests to ensure the reliability, performance, and security of the data aggregator and API.

**Evaluation Criteria:**

- **Functionality:** Does the data aggregator retrieve and process Solana blockchain data accurately and efficiently?
- **Performance:** How well does the application handle large volumes of data and concurrent requests?
- **Reliability:** Is the data aggregator resilient to failures and capable of recovering gracefully?
- **Scalability:** Can the application scale to handle increasing data loads without sacrificing performance?
- **Security:** Are proper security measures implemented to protect data integrity?
- **Documentation and Maintainability:** Is the codebase well-documented, well-composed, maintainable, and easy to understand for future developers?

## Analysis of Requirements

### Unknowns

- How to stream Txs via Solana API / SDK.
- Whether it is pull- or push-based.
- How computationally complex the processing (decoding) of a Tx is.

### High-Level Architecture

- Split up the solution into 2 applications: aggregator service and REST server (however in a monorepo). This decouples the logic and allows independent deployment, therefore simple horizontal scaling, especially in the case of the REST server. Whether scaling of the aggregator service is necessary and/or how it can be done is currently unclear as it is currently unknown how exactly the Txs streaming via Solana API/SDK works and how computantionally expensive it is.
- In case Solana is producing Txs faster than the aggregator can process them, we need to find ways of scaling up the processing, either via simple multi-processing such as splitting the workload over multiple cores via mpsc or by splitting processing functionality out into a separate service, that can be scaled up horizontally, processing TXs to process via a Kafka queue.
- To scale up the REST server we can simply employ a reverse-proxy solution using nginx that round-robins to a number of running REST servers.
- If we observe high load on the REST servers /transactions/:id endpoint we can employ Redis to cache results with a TTL, as Txs are immutable read-only data.
- As persitence solution it makes sense to use a document storage where the transactions and their metadata are stored as json document with one (id) or more keys (datetime). Document based storage (NoSQL) allows for easy horizontal scaling at the cost of eventual consistency, which should be perfectly fine in this use case.

## Plan

1. Examine Solana API / SDK to understand how to stream Txs.
2. Implement Tx aggregator prototype that just streams Txs but doesnt persist them yet.
3. Add persisting of Txs to document storage.
4. Implement REST server, fetching from document storage.
5. If time permits, demonstrate scaling abilities of REST server using nginx.

#### Document Storage

After some brief research the decision was made to use MongoDB as document storage

## Streaming Txs from Solana

There exists the [solana-client crate](https://docs.rs/solana-client/latest/solana_client/) which has a pubsub client module which allows for subscribing to messages from the RPC server. The subscriber provides the `block_subscribe` method to receive a message if a block is confirmed or finalised. These messages contain info about the block, which also contains the vec of _encoded_ transactions.

Unfortunately when testing this against the helius wss it returned "Method not found" which indicates that listening to block updates is disabled on the helius RPC nodes.

https://www.helius.dev/blog/solana-data-streaming
Alternative is to use webhooks provided by helius: https://docs.helius.dev/webhooks-and-websockets/enhanced-websockets

It seems that can be used very easily via helius-rust-sdk https://github.com/helius-labs/helius-rust-sdk
as an example https://github.com/helius-labs/helius-rust-sdk/blob/dev/examples/enhanced_websocket_transactions.rs
