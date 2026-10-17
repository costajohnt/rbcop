a.roles ||= 1
a.touch += 1
a.roles &&= 2
a&.roles ||= 3
a.roles = 4
a.touch
roles ||= 5
a[:x].touch ||= 1
FileUtils.touch ||= 1
a.update_attribute ||= 1
