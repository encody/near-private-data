---
title: Preventing Metadata Leakage in Communication over Public Channels
titlepage: true
author:
  - Jacob Lindahl <lindahl@prg.is.titech.ac.jp>
  - Hidehiko Masuhara <masuhara@prg.is.titech.ac.jp>
papersize: a4
header-includes: |
  \newcommand{\concat}{\mathbin{\|}}
bibliography: ./biblio.bib
---

<!-- ## Comparison with other protocols

| Protocol         | Comparison                   |
| ---------------- | ---------------------------- |
| Matrix (Element) |                              |
| Signal           | Leaks transcript affiliation |
| Session          |                              |
| Mailchain        |                              |
| Telegram         |                              |
| Bitmessage       | Expensive multicast          |
| Molasses         |                              |
| Briar            |                              |
| Ricochet         |                              |
| Tox              |                              |
| Threema          |                              | -->

# Abstract

With trustless technologies becoming ever-more prevalent in the modern Internet, there arises the need for a new type of private messaging protocol: one that preserves the privacy of its users to the fullest extent possible without the need to trust any third party. Current implementations of privacy-focused messaging protocols suffer from a number of problems, ranging from metadata leakage to usability issues. We describe a new message transmission protocol that addresses metadata leakage and broadcast efficiency.

# Definitions

## Metadata

The goal of this paper is to build upon the work of previous protocols to hide even more metadata about conversations. In particular, we will hide the following information, in addition to hiding of the payload itself:

- Sender's identity.
- Sender's location (geographical and network).
- Timestamp of transmission.
- Receiver's identity.
- Receiver's location (geographical and network).
- Timestamp of receipt.
- Payload size.
- Conversation history.
- Forward secrecy.

However, privacy of these data is not sufficient to make a usable protocol. Therefore, we will also ensure the following properties that make the protocol usable:

- Users can easily use the service across multiple devices, including message synchronization.
- Group messaging is efficient and scalable.
- The service is inexpensive to run as a server and as a user/client.

## Conversation history

One of the issues experienced by many protocols in this sector is that while the messaging protocol may be clearly cryptographically and mathematically sound, correctly implementing such fancy techniques as deniability, if transcripts of the conversation are revealed, those hard mathematical evidences do very little to effectively recuse a conversant from a conversation. This has led some protocols to discount such techniques entirely. [@jefferys_session_2020]

The experimental techniques presented in this paper do not endeavor to implement complete deniability in the traditional sense, due in large part to the nature of the invariants required by the infrastructure upon which they depend. That is to say, it would violate the fundamental contract of an "append-only public ledger" if two plausible transcripts could be provided that purport a different sequences of appends.

Rather, we take a different approach. One of the problems with simply implementing something like the Signal Double-Ratchet algorithm is that while it hides the _content_ of the messages between conversants Alice and Bob, it does not hide the fact that Alice and Bob are 1) conversing, or 2) conversing with each other. The flexible channels protocol in itself does attempt to conceal this information. However, it should be duly noted that the protocol as presented assumes the existence of some sort of public-key infrastructure (PKI). PKIs are usually publicly-accessible, so the presence of a user's public key in the PKI could belie their usage of the protocol. This issue can be mitigated somewhat by 1) using a PKI that has sufficient quantity of users for a diverse variety of applications, or 2) not using a PKI, and instead manually facilitating public key exchanges (e.g. by meeting in person, scanning QR codes, etc.).

## Forward secrecy

# Protocol

The key insights of this paper are the channel and sequence hash constructions.

A channel is a total-ordered stream of messages. It consists of membership list (e.g. a sender and a receiver, or multiple members of a group) and a shared secret (derivable by Diffie-Hellman or any other method of establishing a shared secret).

A channel has a deterministic sequence hash generator. Hashing the fields of the channel, together with a nonce, produces a "sequence hash." This sequence hash can only be recreated by parties privy to the channel's shared secret, yet it does not reveal the shared secret. The sequence hash serves as the identifying key of a message sent to the message repository.

The message repository is a simple construct, consisting only of a key-value store from which anyone can read, and to which anyone can write. The only restriction is that existing keys cannot be overwritten.

Using a series of abstractions, the channel construct can be composed in a number of different ways, supporting bidirectional $1 \leftrightarrow 1$ messaging, $1 \leftrightarrow N$ messaging, and $N \leftrightarrow M$ messaging.

Of particular interest is $N \leftrightarrow M$ messaging. If all members of a particular group share the same secret, they can all generate the same sequence of hashes, and decrypt all of the messages in the channel. This allows for posting a single message, encrypted with the shared secret, to the public message repository, and all of the members of the group will be able to read it, regardless of the number of members in the group. Thus, we have $O(1)$ space complexity for broadcast transmissions.

## Channels

Consider a unidirectional channel $c$, with sender $s$ and receiver $r$, where the public keys of $r$ and $s$ are mutually known. Using Diffie-Hellman, or any other method of establishing a shared secret, $s$ and $r$ can establish a shared secret $k$. In combination with other, optional, static metadata, a similarly secret channel identifier $i_{s \rightarrow r}$ can be derived. An example of this process is demonstrated below:

$$ i_{s \rightarrow r} = H(k, s \concat r) $$

where $H$ is a keyed cryptographic hash function (such as HMAC-SHA3-512), $s$ is the public key of the sender, $r$ is the public key of the receiver, and $k$ is the shared secret.

The channel identifier $i_{s \rightarrow r}$ is used to identify the channel $c$. However, this identifier must remain secret, as it can be used to derive the sequence hashes for the channel before they have been posted to a public message repository.

## Generating sequence hashes

Once a channel has been established, the sender and receiver can generate a sequence hash $h_{s \rightarrow r}^n$ for the $n$th message in the channel. This sequence hash is used to identify the message in the message repository. The sequence hash is generated as follows:

$$ h_{s \rightarrow r}^n = H(i_{s \rightarrow r}, n) $$

where $n$ is the sequence number of the message.

## Posting and reading messages

Once a sequence hash has been generated, the sender can post a payload $(h_{s \rightarrow r}^n, c)$ to the message repository, where $c$ is the ciphertext of the message, encrypted with the shared secret $k$. Once the message has posted, the receiver, knowing the shared secret $k$ and the index of last message he saw $n-1$, can find the message keyed by $h_{s \rightarrow r}^n$ in the message repository, and decrypt it.

# Development

A reference implementation for this paper is currently in development. The following is a proposal for the development of the protocol.

## Phase 1: Proof of Concept

The basic proof of concept has already been completed and shown to work in a limited capacity. The proof of concept is a simple command-line application that allows for $1:1$ messaging between two parties. The proof of concept is written in Rust, and uses the _Dalek_ libraries for cryptographic primitives, as well as _ChaCha20-Poly1305_ for encryption.

## Phase 2: Multi-Device & Group Messaging

The next phase of development will focus on multi-device messaging and group messaging. This will be implemented in the proof of concept, and will be written in Rust.

## Phase 3: Library

The final phase of development will be to create a library that can be used by other applications. This will be written in Rust, and will be published to crates.io.

\newpage

# References
