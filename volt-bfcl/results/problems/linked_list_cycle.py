def has_cycle(head):
    slow, fast = head, head
    while fast and fast.next:
        slow = slow.next
        fast = fast.next.next
        if slow == fast:
            return True
    return False

class ListNode:
    def __init__(self, x):
        self.val = x
        self.next = None
head = ListNode(3)
head.next = ListNode(2)
head.next.next = ListNode(0)
head.next.next.next = ListNode(-4)
head.next.next.next.next = head.next
print(has_cycle(head))
head2 = ListNode(1)
head2.next = ListNode(2)
head2.next.next = head2
print(has_cycle(head2))
head3 = ListNode(1)
print(has_cycle(head3))