class Logs
  Rails.prepend(Module.new { def logger = (@logger ||= Logger.new(IO::NULL)) })

  def setup
    Logger.new(
      type: :rails,
      customize_event:
        lambda { |event| event["db"] = 1 },
      level: 1,
    )
  end
end
