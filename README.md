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

### Document Storage

After some brief research the decision was made to use MongoDB as document storage. MongoDB is very popular and has a reach feature set, e.g. compared to CouchDB which is older.
An alternative would be to simply rely on Postgres, however for this use case it seems like a document storage is better suited as its much easier to interact with, has more powerful querying ability and scales better horizontally.

## Plan

1. DONE Examine Solana API / SDK to understand how to stream Txs.
2. DONE Implement Tx aggregator prototype that just streams Txs but doesnt persist them yet.
3. DONE Add persisting of Txs to document storage.
4. DONE Implement prototype of REST server, fetching from document storage.
5. DONE Implement fetching transaction by id.
6. DONE Implement fetching transaction by day timestamp.
7. DONE Scale up/parallelise aggregator service so it can catch up with network.
8. DONE Clean up code base, fixing unwraps and error handling, adding doc comments.
9. TODO add streaming of account info (probably possible via callbacks)
10. TODO add fetching of account info from new REST endpoints

## Solving Streaming Txs from Solana

There exists the [solana-client crate](https://docs.rs/solana-client/latest/solana_client/) which has a pubsub client module which allows for subscribing to messages from the RPC server. The subscriber provides the `block_subscribe` method to receive a message if a block is confirmed or finalised. These messages contain info about the block, which also contains the vec of _encoded_ transactions.

Unfortunately when testing this against the helius wss it returned "Method not found" which indicates that listening to block updates is disabled on the helius RPC nodes.

Trying to use the helius ws transaction subscription (see https://github.com/helius-labs/helius-rust-sdk
as an example https://github.com/helius-labs/helius-rust-sdk/blob/dev/examples/enhanced_websocket_transactions.rs) seemed also not to be the right solution as it essentially filters for transactions, but we want all of them.

The next attempt was to use the solana client slot subscription to get notifications of newly processed slots and use get_block to fetch the corresponding block. This initially caused problems but after switching to the nonblocking version notifications of new slots worked fine. However (obviously) this didn't work either, because finalising of slots by the network takes a while, so when fetching the block to a slot immediately when the slot was produced, results in "Block not available for slot ...".

Next solution is to try a polling solution by fetching blocks for finalised slots.

1. start with latest finalised slot using get_slot
2. after a timeout of e.g. 1 seconds get the latest finalised blocks with get_blocks
3. fetch each block for the returned blocks (slots)

As it seems the polling solution does work.

Note that a polling solution is not ideal especially in a setting where we have such high frequency of new blocks as in Solana, where we can risk of falling behind if we dont poll fast enough. However given that the pushing (pubsub) mechanisms all didn't work (and we were running out of time to research other solutions) we decided to stick with this solution for now. From running some short tests it seems that the blocks that are returned by the `get_blocks` call is on average always the same, so this means we can keep up with the block production time of Solana. If however the list of new finalised blocks keeps growing each time, then we know we are processing new blocks slower than the network produces them, and therefore we need to find ways of scaling this up, probably by fetching blocks in parallel using an mcsp channel and e.g. 2-4 threads.

Obviously also there is a balance to strike between how realtime the transactions/blocks should be and how many requests we make. Currently we fetch new blocks every 1_000 millseconds, which seems to be a good balance, however this can be change easily if tighter realtime polling is required and more generous Helius plans are available.

## Testing

Generally tests are written following a "testing pyramid", that consists of 3 layers, bottom to top:

1. A large number of unit tests, that execute fast, and cover complex domain logic.
2. A number of integration tests, that take longer to execute and cover integration between modules as well as with infrastructure. Integration tests can be split into mocked tests where we are dealing with mocks, therefore we can call them something like unit tests; and full integration tests, where the code under test runs against some test infrastructure such as a local MongoDB instance.
3. A few E2E tests that test the system via its external endpoints and check the observable effects.

I didn't implement any tests in this case study for the following reasons:

- There isnt any complex domain logic that needs test coverage, as the main complexity of this case study lies in the interaction with the infrastructure.
- When dealing with complex integration we can employ mocks, full integration tests and E2E tests, all of which I deemed outside the scope of this case study.

Possible integration and E2E tests:

- Mock MongoDb with a trait and write unit (integration) tests for the REST handler
- Testing the aggregator with integration tests doesnt make much sense, rather focus on E2E tests here
  - Start a fresh (empty) local MongoDB.
  - Configure aggregator to fetch a single given block which we know what is in it.
  - Fetch txs we know are in the block from REST endpoint and compare with expected data.

## Running

1. Start MongoDB by running `start_mongo.sh`
2. Make a copy of `config/config.aggregator.example.sh` to `config/config.aggregator.sh` and configure `RPC_URL` to a corresponding RPC Solana node endpoint.
3. Start aggregator service by running `start_aggregator.sh`
4. Make a copy of `config/config.server.example.sh` to `config/config.server.sh`. No need to configure anything, the defaults should be fine.
5. Start the REST server by running `start_server.sh`. By default it runs on localhost port 3000.
6. Fetch transactions by id: `curl http://localhost:3000/transactions?id=58MKzFPeW6syG6unZT8Rsfn4yeKf1vtiH6XGAoj93xg4B6DKpi2ZeJuYU7zu1rbnBCGgQhftDZczYAPSHwHsrt3Z`
7. Fetch transactions by day: `curl http://localhost:3000/transactions?day=31/07/2024`
8. Fetch accunt by id: `curl http://localhost:3000/account/CTTHnWEM7WRQVp9gTZL2SDT1vtGrgu9hYEgtJhjwhDHx`

## Time tracking

~ 3 hours for Solana Tx fetching
~ 2 hours for adding MongoDB
~ 2 hours for setting up REST server and implementing endpoints
~ 1 hour for parallelising the Tx processing
~ 1 hour of cleaning up (comments, unwrap, additional documentation)
~ 2 hours of adding Accounts fetching and REST endpoint
