## Search Algorithms

### [Linear](./linear_search.rs)

![alt text][linear-image]

From [Wikipedia][linear-wiki]: linear search or sequential search is a method for finding a target value within a list. It sequentially checks each element of the list for the target value until a match is found or until all the elements have been searched.
Linear search runs in at worst linear time and makes at most n comparisons, where n is the length of the list.

**Properties**

- Worst case performance O(n)
- Best case performance O(1)
- Average case performance O(n)
- Worst case space complexity O(1) iterative

### [Binary](./binary_search.rs)

![alt text][binary-image]

From [Wikipedia][binary-wiki]: Binary search, also known as half-interval search or logarithmic search, is a search algorithm that finds the position of a target value within a sorted array. It compares the target value to the middle element of the array; if they are unequal, the half in which the target cannot lie is eliminated and the search continues on the remaining half until it is successful.

**Properties**

- Worst case performance O(log n)
- Best case performance O(1)
- Average case performance O(log n)
- Worst case space complexity O(1)

### [Exponential](./exponential_search.rs)

![alt text][exponential-image]

From [Wikipedia][exponential-wiki]: Exponential search allows for searching through a sorted, unbounded list for a specified input value (the search "key"). The algorithm consists of two stages. The first stage determines a range in which the search key would reside if it were in the list. In the second stage, a binary search is performed on this range. In the first stage, assuming that the list is sorted in ascending order, the algorithm looks for the first exponent, j, where the value 2^j is greater than the search key. This value, 2^j becomes the upper bound for the binary search with the previous power of 2, 2^(j - 1), being the lower bound for the binary search.

**Properties**

- Worst case performance O(log i)
- Best case performance O(1)
- Average case performance O(log i)
- Worst case space complexity O(1)

### [Jump](./jump_search.rs)

![alt text][jump-image]

From [Wikipedia][jump-wiki]: In computer science, a jump search or block search refers to a search algorithm for ordered lists. It works by first checking all items L(km), where k ∈ N and m is the block size, until an item is found that is larger than the search key. To find the exact position of the search key in the list a linear search is performed on the sublist L[(k-1)m, km].

**Properties**

- Worst case performance O(√n)
- Best case performance O(1)
- Average case performance O(√n)
- Worst case space complexity O(1)

### [Fibonacci](./fibonacci_search.rs)

From [Wikipedia][fibonacci-wiki]: In computer science, the Fibonacci search technique is a method of searching a sorted array using a divide and conquer algorithm that narrows down possible locations with the aid of Fibonacci numbers. Compared to binary search where the sorted array is divided into two equal-sized parts, one of which is examined further, Fibonacci search divides the array into two parts that have sizes that are consecutive Fibonacci numbers.

**Properties**

- Worst case performance O(log n)
- Best case performance O(1)
- Average case performance O(log n)
- Worst case space complexity O(1)

[linear-wiki]: https:
[linear-image]: http:
[binary-wiki]: https:
[binary-image]: https:
[exponential-wiki]: https:
[exponential-image]: https:
[jump-wiki]: https:
[jump-image]: https:
[fibonacci-wiki]: https:
