---
hidePagination: false
url: "/dist-system-en/forward/"
title: "The Charm of Distributed Systems"
description: "Foreword: the charm of distributed systems and why understanding them remains essential in the AI era."
date: 2026-06-25
tags: ["distributed systems"]
categories: ["distributed systems"]
keywords:
- Distributed Systems
- Partial Failure
- Consensus
- CAP Theorem
weight: -1
toc: true
---

This is a story about "how to build reliable systems out of unreliable components."

When you open your phone on a rainy night to order food delivery, the system behind it is undergoing complex collaboration: your request passes through a load balancer and gets routed to an instance of the order service; the order data is persisted to the master node of the database and synchronized to two replica nodes across datacenters in milliseconds; at the same time, the inventory service in another cluster is deducting stock, and the payment system is calling a third-party gateway. If any step fails, a series of compensation logics will automatically trigger. The whole process spans dozens of servers, unreliable network links, and hardware that might strike at any time—but in most cases, your food still arrives on time.

This is the daily routine of distributed systems. It looks like magic, but behind it lies decades of the most exquisite engineering practices in computer science.

## The Root of Complexity: Confronting Physical Constraints

Transitioning from single-machine to distributed, the most fundamental paradigm shift is: we must learn to coexist with **Partial Failure**.

In a single-machine system, the machine is either running normally or completely dead. But in a distributed system, the system is always in a Schrödinger-like "half-dead" state: a small fraction of nodes crash, or the network partitions, yet the system as a whole keeps running. More fatally, **in an asynchronous network, you can never distinguish whether a node has completely crashed or is merely responding slowly**.

This seemingly simple physical constraint breeds almost all the complexities of distributed systems:

- Network unreliability: Messages can be lost, delayed, reordered, or even duplicated. You might think a sent request has succeeded, while in reality, it might be stuck queuing in a router's buffer.
- Node failures: Hardware aging, memory errors, OS kernel crashes... any node can unexpectedly go down at any time. What's more troublesome is that it might rejoin the cluster with an inconsistent state after experiencing "amnesia."
- Absence of a global clock: The physical clocks of each machine drift at different rates. You cannot simply rely on "the larger timestamp holds the latest state" to resolve conflicts, because in a distributed world, time across different machines is fundamentally incomparable globally.
- Concurrency is everywhere: Hundreds of thousands of clients read and write data simultaneously, multiple nodes run for leader concurrently, and Network Partitions cause different parts of the same cluster to act independently. Correctly reasoning about and handling these concurrent behaviors is the most demanding test of fundamental skills in system design.

In the single-machine world, we have an absolutely reliable "source of truth"—the state in memory and disk is deterministic. But in distributed systems, **there is no single source of truth**; each node can only see a local view of the world, like blind men feeling an elephant. How to make these nodes, which only possess local information, piece together a globally consistent state is the core challenge of distributed systems.

## The Beauty of Engineering Under Constraints

If distributed systems were merely "difficult," it wouldn't attract so many engineers and researchers to dedicate themselves to it. Its true charm lies in this: facing these seemingly unsolvable physical constraints, humanity has designed extremely elegant solutions.

The **Quorum** mechanism is a great example. Its mathematical foundation is very simple—"any two majority sets must intersect"—but starting from this humble property, we built replication systems that tolerate minority node failures and designed consensus algorithms that guarantee log consistency. Transforming chaotic uncertainty into seamless determinism is itself a process full of intellectual joy.

The **evolution of consensus algorithms** is equally astonishing. Leslie Lamport's **Paxos** started from a seemingly impossible goal, and through two rounds of simple quorum read-write interactions, provided a rigorous mathematical solution for "how to reach consensus over an unreliable network." The subsequent **Raft** algorithm demystified esoteric theory into an engineering blueprint, allowing strong consistency systems to truly enter the production environments of thousands of households.

The **CAP theorem** demonstrates the beauty of trade-offs in distributed systems, where "you can't have your cake and eat it too." It does not dogmatically prescribe "what can be done," but clearly delineates "what is absolutely impossible." It is exactly these Impossibility Results that establish the boundaries of system design, letting us know where we must compromise and where we can boldly innovate.

This process of finding optimal solutions under severe constraints is the essence of engineering design.

## Why Do We Still Need to Learn Distributed Systems in the AI Era?

You might ask: we are now in the age of artificial intelligence, and Large Language Models (LLMs) are reshaping the boundaries of software engineering. Why should we still painstakingly study these foundational distributed theories?

The answer is very simple: modern AI technology is fundamentally built upon the largest-scale distributed systems in human history.

When we talk about hundred-billion-parameter large models, we are no longer talking about algorithms on a single machine, but massive computing clusters composed of tens of thousands of GPUs. In this cluster, the core phantom of distributed systems—Partial Failure—has not disappeared; instead, it is infinitely magnified.

Imagine this: during a model training session spanning months and utilizing tens of thousands of graphics cards, if a certain server's network card experiences microsecond-level jitter, or a specific GPU suddenly suffers a memory fault, how should the entire training task proceed? Without efficient distributed fault-tolerance mechanisms, and without exquisite distributed Snapshot and Checkpoint technologies, training models with hundreds of billions of parameters would be utterly impossible.

Taking a step further, every piece of infrastructure in the AI era has the DNA of distributed systems flowing in its blood:

- Compute scheduling: How do thousands of compute nodes collaborate? Whether it is data parallelism, tensor parallelism, or pipeline parallelism, they are essentially solving Partition and communication problems in distributed systems.
- Massive data: The Vector Databases and distributed object storage that support the AI explosion still rely on Raft or Paxos at the bottom level to guarantee the strong consistency of metadata, and rely on multi-replica Replication to ensure high data availability.
- Global inference: When hundreds of millions of users simultaneously initiate conversations with large models, how do we provide low-latency responses globally through distributed routing, load balancing, and distributed sharing of KV Cache? This is exactly the classic battlefield of distributed concurrency and state management.

If neural networks and deep learning algorithms are the "brains" of AI, then distributed systems are the "bones and nerves" supporting this brain's operation. No matter how the upper-layer applications of AI change, as long as it still needs to run on machines in the physical world, it will never escape the cold physical constraints of network latency, node crashes, and physical clock drift.

Therefore, in this era swept by the AI wave, mastering distributed systems is not outdated; on the contrary, it has become the scarcest and most hardcore capability. It allows you not only to remain at the user level of "calling AI APIs," but to possess the confidence of becoming an "AI infrastructure architect," capable of taming those massive computing behemoths that truly drive the future intelligent world.

## How This Book is Organized

This book features "illustrations" as its core characteristic, striving to help readers truly understand the underlying principles of distributed systems through intuitive diagrams and logical deduction, rather than rote memorizing dry algorithms. The book's arrangement is not a simple listing of chapters but follows a bottom-up construction logic of "from physical constraints to business abstraction":

- **Overview of Distributed Systems**: Starting from the definition of distributed systems, we compare them with single-machine systems dimension by dimension, revealing their core challenges: unreliable networks, broken clocks, partial failures, and the resulting data consistency issues. The chapter concludes by discussing the mental shift required from single-machine to distributed thinking—from two-state logic to three-state, from absolute time to relative order, and from perfectionism to the art of trade-offs.

- **Distributed System Models**: Starting from two classic thought experiments—the Two Generals' Problem and the Byzantine Generals' Problem—we establish the theoretical foundation for distributed system design. Subsequently, we define the three core models: communication models (fair-loss links, reliable links, authenticated links), failure models (the hierarchy from crash-stop to Byzantine), and timing models (synchronous, asynchronous, partially synchronous), providing a precise environmental assumption language for the algorithm design in subsequent chapters.

- **Time and Order in Distributed Systems**: Time is the most fundamental puzzle in distributed systems. This chapter starts with a seemingly simple question—"how to determine the sequence of events in a distributed system"—and gradually reveals why physical clocks are inadequate for this task. We start from the physical principles of quartz and atomic clocks, go through the engineering disasters brought by UTC leap seconds and the flawed symmetry assumptions of the NTP protocol, to deduce the ineliminability of clock skew and drift. Ultimately, we arrive at a surprising conclusion: in distributed systems, "causal order" is more fundamental than "physical time." Revolving around this insight, the chapter introduces Lamport's Happens-Before relationship to precisely define causality between events, uses the mathematical language of partial order and total order to explain why certain events are inherently unorderable, and finally derives two logical clocks—Lamport clocks and Vector clocks—as tools to capture causality without relying on a global clock. This chapter also discusses the relationships among state, events, and snapshots, laying the theoretical groundwork for state machine replication and consensus algorithms in subsequent chapters.

- **Replication**: Replication is the basic means to solve single-point vulnerability. This chapter introduces leader-follower replication (synchronous, asynchronous, semi-synchronous) and quorum read-write mechanisms, systematically outlines the complete consistency model spectrum from linearizability to eventual consistency, and deeply analyzes the CAP theorem and its improved form PACELC, elucidating the trade-off between consistency and availability.

- **Distributed Consensus Algorithms**: Starting from the FLP impossibility theorem, we progressively deduce the core ideas of the Paxos algorithm evolving from quorum read-writes, and then detail the complete design of the Raft algorithm—leader election, log replication, safety guarantees, membership changes, liveness optimization, and log compaction. This is the capstone chapter of the entire book.

- **Partitioning**: If replication is for fault tolerance, partitioning is for scalability. This chapter explains data sharding techniques, including engineering practices like consistent hashing principles and virtual node optimization, solving the horizontal scaling problem when single-machine capacity is insufficient to hold all data.

- **Transactions**: After mastering the underlying replication and partitioning, we will return to the application perspective. Starting from single-machine ACID properties, this chapter analyzes Undo/Redo log mechanisms and concurrency control (2PL, OCC, MVCC, Write Skew), extending to distributed transaction solutions like 2PC/3PC, Google Spanner's TrueTime innovation, and flexible transaction schemes like TCC and SAGA.

It is recommended to read in order for the first time, as advanced architectures are always built upon solid cornerstones. Later chapters will frequently reference earlier concepts.

## Who This Book is For

This book is suitable for the following readers:

- Backend engineers and architects with some programming experience, eager to break through their technical ceilings and dive deep into the distributed foundations.
- College students studying distributed systems courses, hoping to find a practical reference book with more engineering intuition than traditional textbooks and more accessibility than academic papers.
- R&D personnel preparing for System Design interviews, who need to build a solid understanding of core algorithms and consistency models.
- Any tech geek curious about "how large-scale internet systems operate."

This book assumes readers have basic computer science literacy (such as data structures, operating system principles, computer networks), but does not require prior knowledge of distributed systems. When encountering necessary mathematical and theoretical concepts, we will explain them thoroughly and accessibly within the context.

## Final Words

Distributed systems is a field where theory and engineering are highly intertwined. Algorithms in papers often have elegant proofs, but in real production environments, we must face various realistic failure issues like network jitter, silent disk errors, and even datacenter power outages. There is a massive chasm between deriving the Paxos algorithm on a whiteboard and actually writing industrial-grade code that can withstand the tests of a production environment.

The original intention of writing this book is to build a bridge over this chasm. I try to filter out overly obscure academic expressions, using the language engineers are familiar with and intuitive diagrams to peel back and thoroughly explain those time-tested classic principles layer by layer.

In this field, there is no universally applicable "silver bullet," only trade-offs and choices based on profound understanding. If, while reading the source code of an open-source project, you can suddenly see the light because of a diagram in this book; or when facing complex system architecture designs, you can have a bit more confidence in moving from ideal to implementation, then my goal in writing this book will have been achieved.

The road to exploring distributed systems is full of challenges, but equally full of joy. I hope this little book can become your steadfast and reliable companion on this journey.

<div align="right">
Lichuang<br>
2025
</div>
