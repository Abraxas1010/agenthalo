namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace Protocol

def topicPrefix : String := "/nucleusdb/capabilities/"

def allowedTopicSuffixes : List String :=
  ["general", "coding", "research", "financial", "blockchain", "privacy"]

def allowedTopics : List String :=
  allowedTopicSuffixes.map (fun suffix => topicPrefix ++ suffix)

def isAllowedTopic (topic : String) : Prop :=
  topic ∈ allowedTopics

theorem known_topics_are_allowed :
    ∀ suffix ∈ allowedTopicSuffixes, isAllowedTopic (topicPrefix ++ suffix) := by
  intro suffix hs
  unfold isAllowedTopic allowedTopics
  exact List.mem_map.mpr ⟨suffix, hs, rfl⟩

theorem rejected_topic_not_in_allowed_list (topic : String)
    (h : topic ∉ allowedTopics) :
    ¬ isAllowedTopic topic := by
  simpa [isAllowedTopic] using h

end Protocol
end Comms
end NucleusDB
end HeytingLean
