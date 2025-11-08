




use crate::data_structures::linked_list::LinkedList; 

pub fn detect_cycle<T>(linked_list: &LinkedList<T>) -> Option<usize> {
    let mut current = linked_list.head;
    let mut checkpoint = linked_list.head;
    let mut steps_until_reset = 1;
    let mut times_reset = 0;

    while let Some(node) = current {
        steps_until_reset -= 1;
        if steps_until_reset == 0 {
            checkpoint = current;
            times_reset += 1;
            steps_until_reset = 1 << times_reset; 
        }

        unsafe {
            let node_ptr = node.as_ptr();
            let next = (*node_ptr).next;
            current = next;
        }
        if current == checkpoint {
            return Some(linked_list.length as usize);
        }
    }

    None
}

pub fn has_cycle<T>(linked_list: &LinkedList<T>) -> bool {
    let mut slow = linked_list.head;
    let mut fast = linked_list.head;

    while let (Some(slow_node), Some(fast_node)) = (slow, fast) {
        unsafe {
            slow = slow_node.as_ref().next;
            fast = fast_node.as_ref().next;

            if let Some(fast_next) = fast {
                
                fast = fast_next.as_ref().next;
            } else {
                return false; 
            }

            if slow == fast {
                return true; 
            }
        }
    }
    
    false 
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_cycle_no_cycle() {
        let mut linked_list = LinkedList::new();
        linked_list.insert_at_tail(1);
        linked_list.insert_at_tail(2);
        linked_list.insert_at_tail(3);

        assert!(!has_cycle(&linked_list));

        assert_eq!(detect_cycle(&linked_list), None);
    }

    #[test]
    fn test_detect_cycle_with_cycle() {
        let mut linked_list = LinkedList::new();
        linked_list.insert_at_tail(1);
        linked_list.insert_at_tail(2);
        linked_list.insert_at_tail(3);

        
        unsafe {
            if let Some(mut tail) = linked_list.tail {
                if let Some(head) = linked_list.head {
                    tail.as_mut().next = Some(head);
                }
            }
        }

        assert!(has_cycle(&linked_list));
        assert_eq!(detect_cycle(&linked_list), Some(3));
    }
}
