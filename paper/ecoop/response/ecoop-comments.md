### Review #44A

#### Response to comment: The description of how the analysis works

Response: Thank you for highlighting the need for greater clarity in explaining our analysis. According to the opinion, in order to clearly express our analysis process, we change the original chapter structure to: (1) pointer analysis; (2) The construction of extended lock graph; (3) Deadlock detection algorithm.

1. Pointer analysis: Pointer analysis is the basis of our analysis method, which gives the relationship of passing aliases between procedures and within procedures (Table 1 in the paper). The mode of operation of passing locks in a Java context or sharing lock variables among class members is not different from Rust, but instead of using critical sections, Rust puts the data that needs to be locked inside the lock and returns a MutexGuard, that is, The construction and release of MutexGuard corresponds to the scope of a critical section in Java. The improvements in Rust's ownership and lifecycle mechanisms are mainly in two areas. Ownership ensures that if we pass variables instead of references in the calling context, we don't need to construct point-to relationships; Lifetime guarantees that locks passed by Pointers do not need to be constructed if they are not in the same lifetime. The changes in these two aspects are mainly reflected in the construction of lock graph, aiming to reduce false positives. After pointer analysis, we obtain a constraint graph ConsG that contains the alias relations of the locks.
2. Construction of lock graphs: Regarding lock graphs, we define two different types of nodes based on deadlock patterns, distinguished by node properties. First for locks, we denote as node G. For condition variables, the wait and notify operations are tabulated as nodes MT and NT, respectively. The construction of lock graph needs to ensure the control flow correctness of the program, so our method is based on the function call graph. Each function in the function call graph is traversed, and the statements containing locks or condition variables are transformed accordingly. To ensure the control flow of the function, empty nodes are used to fill the statements that do not contain locks and ensure the transfer validity of control dependencies. In a graph containing only lock structures, each node is traversed, the Alias relationship with other lock nodes is judged, and an alias edge is added if an alias exists.
3. Deadlock detection algorithm: For two different deadlock patterns, we construct two kinds of lock graphs to detect them. For deadlock patterns that do not contain condition variables, we consider that if there is a ring in the lock graph, then a deadlock is reported, if the lock in the ring is a Mutex, then this deadlock type is a double lock, if it is a read-write lock, then a conflict deadlock.

### Review #44B

#### Response to comment: How does the approach address the Rust standard library (which has a lot of unsafe code)?

Response: Thank you for your helpful comments. In our previous investigation, we found that all deadlock patterns in Rust are found in safe code, and unsafe code mainly targets memory errors.

#### Response to comment: How were the benchmark programs chosen? Were there programs on which it failed to work?

Response: Thank you for your insightful comments. Part of the test cases are derived from lockbud, and the other part is manually constructed based on the deadlock patterns proposed in this paper. These programs compile and run.

#### Response to comment: Are there unsupported language features in MIR (and what do they translate to in Rust)?

Response: Thank you for your insightful comments. To the best of our knowledge, there is no unsupported language function in MIR.

#### Response to comment: What happens when this article is applied to asynchronous code?

Response: Thank you for your insightful comments. In MIR, async functions are translated into a state machine, which is represented by multiple generated MIR blocks. These MIR blocks collectively handle the different asynchronous states of the function. This is very challenging for pointer analysis and lock graph construction. So our method does not yet support deadlock detection for asynchronous code such as Tokio.

### Review #44C

#### Response to comment: Why the authors don’t pursue this strategy is a little insubstantial

Response: Thank you for pointing out the need for a more detailed explanation regarding our decision not to use LLVM IR.  We agree that this requires a thorough justification.  Here are the detailed reasons:

1. Lack of High-Level Semantic Information: LLVM IR is a lower-level intermediate representation designed for optimization and code generation.  While it abstracts machine-specific details, it lacks high-level semantic information present in Rust, such as ownership, lifetimes, and borrowing rules.  These features are crucial for accurately analyzing deadlock conditions in Rust, as they directly influence how resources are managed and synchronized.

2. Complexity of Mapping High-Level Constructs: Translating Rust's high-level constructs into LLVM IR can result in the loss of critical information needed for precise deadlock analysis.  For example, Rust’s ownership model and borrow checker enforce strict rules that prevent data races and certain classes of deadlocks at compile time.  These constructs do not have direct equivalents in LLVM IR, making it challenging to perform a meaningful analysis without this semantic context.

3. Potential Misalignment with Rust's Concurrency Model: Rust's concurrency model, which leverages its ownership and type systems to enforce thread safety, is not inherently reflected in LLVM IR.  This could lead to scenarios where the analysis performed at the LLVM IR level misses critical concurrency issues specific to Rust, thus reducing the effectiveness of the analysis.

## Response to comment: Minor comments

1. (30): "Rust further extends this fundamental rule with measures to ensure memory and thread safety." This brief explanation misses the point a little. The "fundamental rule" of ownership itself guarantees memory and thread safety; the extra measures Rust adds on top allow more flexibility for the programmer while still preserving these guarantees. (This is explained better later on!)
    Response: We agree with your observation. We have revised the sentence to better reflect the role of Rust's ownership system. The revised text now reads: "Rust's fundamental ownership rule inherently guarantees memory and thread safety. The additional measures provide more flexibility for the programmer while maintaining these guarantees."
2. (51): This paragraph packs a lot into a small space - maybe expand out into bullet points or similar?
    Response: We have expanded this paragraph into bullet points to improve clarity and readability.
3. (96): Some more explanation of these points would be good.
    Response: We have added additional explanations to this section to provide a clearer understanding of the points discussed.
4. (118): "Conflict lock is the primary type of deadlock detection" -> "Conflict lock is the primary type of deadlock"
    Response: We have corrected this sentence to: "Conflict lock is the primary type of deadlock."
5. (134): "The Framework of Method" slightly confusing - maybe "Our Method"?
    Response: We have revised the title to "Our Method".
6. (138): Figure 1 is quite distant from this!
    Response: We have repositioned Figure 1 to be closer to the related text for better coherence.
7. (149): "Utilize" -> "We utilize" (or "We use")
    Response: We have changed "Utilize" to "We use" for better readability.
8. (185): "If the statement in the main() function, beta is empty" sentence fragment - I’m not sure what was intended here
    Response: We have clarified this sentence to: "If the statement in the main() function indicates that beta is empty, then..."
9. (214): "the set of other guard variables alive current" -> "the set of other guard variables currently alive"
    Response: We have corrected this phrase to: "the set of other guard variables currently alive."
10. (222): "and their lifetimes overlap, check" -> "and their lifetimes overlap, we check"
    Response: We have revised this sentence to: "and their lifetimes overlap, we check."
11. (238): "And through pointer analysis," is this sentence missing the beginning?
    Response: We have completed this sentence to: "Through pointer analysis, we can determine..."
12. (243): between theta and what?
    Response: We have coorected this phrase to: "according to \theta"
13. (245): the spacing between eg MM and RW looks slightly odd
    Response: We have adjusted the spacing for better formatting.
14. (265) and (266): "edges be added" -> "edges will be added" or similar
    Response: We have corrected these phrases to: "edges will be added."
15. (336): It would be nice if this limitations section could be expanded substantially, particularly due to the large number of points relegated to future work that are mentioned throughout the paper - maybe these ought to be collected here.
    Response: We have expanded the limitations section to include a more comprehensive discussion of the limitations and future work mentioned throughout the paper.
Thank you again for your constructive feedback. We believe these changes will significantly improve the clarity and quality of our paper.
