class SocketTest < TestCase
  class TestSocket < Socket
    class TestConnection
      def initialize(socket)
        @socket = socket
      end

      def handle_open = @socket.connect
    end

    def connect
      @connected = true
    end
  end
end
