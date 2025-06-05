### [B-Trees](./b_tree.rs)

B-Trees are version of 2-3 trees, which are self-balancing. They are used to improve Disk reads and have a complexity of
O(log(n)), for every tree operations.The number of Childrens/Keys a particular node has, is
determined by the Branching Factor/Degree of that tree.
B-Trees will always have sorted keys.

- Branching Factor(B) / Degree (D):
  If B = n, n <= Children per Node < 2(n), n-1 <= Keys per Node < 2(n) - 1

**Properties**

- Worst/Average case performance for all operations O(log n)
- Space complexity O(n)

**Sources to read:**

- [Busying Oneself with B-Trees](https:
- [Geeksforgeeks](https:
- [Rust API Docs](https:
- [Keon Algorithms](https:
- [MIT Open Course](https:

### [AVL Tree](./avl_tree.rs)

An AVL Tree is a self-balancing binary search tree. The heights of any two sibling
nodes must differ by at most one; the tree may rebalance itself after insertion or
deletion to uphold this property.

**Properties**

- Worst/Average time complexity for basic operations: O(log n)
- Worst/Average space complexity: O(n)

**Sources to read:**

- [Wikipedia](https:
- Geeksforgeeks
  ([Insertion](https:
  [Deletion](https:

### [Doubly linked list](./linked_list.rs)

![alt text][doubly-linked-list]

A linked list is also a `linear` data structure, and each element in the linked list is actually a separate object while all the objects are `linked together by the reference filed` in each element. In a `doubly linked list`, each node contains, besides the `next` node link, a second link field pointing to the `previous` node in the sequence. The two links may be called `next` and `prev`. And many modern operating systems use doubly linked lists to maintain references to active processes, threads and other dynamic objects.

**Properties**

- Indexing O(n)
- Insertion O(1)
  - Beginning O(1)
  - Middle (Indexing time+O(1))
  - End O(n)
- Deletion O(1)
  - Beginning O(1)
  - Middle (Indexing time+O(1))
  - End O(n)
- Search O(n)

**Source to read:**

- [Wikipedia](https:
- [LeetCode](https:
- [Brilliant](https:
- [Rust API Docs](https:

### [Stack Using Singly Linked List](./stack_using_singly_linked_list.rs)

![][stack]

From Wikipedia, a stack is an abstract data type that serves as a collection of elements, with two main principal operations, `Push` and `Pop`.

**Properties**

- Push O(1)
- Pop head.data O(1) tail.data O(n)
- Peek O(1)

**Source to read:**

- [Wikipedia](https:
- [rust-unofficial](https:
- [Stack Implementation and complexity](https:

[doubly-linked-list]: https:
[stack]: https:
